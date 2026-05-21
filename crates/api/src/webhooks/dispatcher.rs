//! Webhook dispatcher.
//!
//! Outbox -> webhook_deliveries fan-out:
//!   1. Find outbox_events with dispatched_at IS NULL.
//!   2. For each, materialize a webhook_deliveries row per active endpoint.
//!   3. Mark outbox dispatched.
//!
//! Then drive deliveries:
//!   - Find webhook_deliveries with status='pending' AND next_attempt_at <= now().
//!   - POST signed payload. On 2xx -> delivered. On other -> backoff schedule.
//!
//! Backoff: 30s, 2m, 10m, 1h, 6h, 24h. After 6 attempts -> failed.
//!
//! State changes write to outbox in the SAME transaction as the change. So the
//! API never blocks waiting for HTTP. The worker handles delivery off the
//! request path.

use crate::webhooks::signer::sign;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;
use uuid::Uuid;

const BACKOFF_SECONDS: &[i64] = &[30, 120, 600, 3_600, 21_600, 86_400];
const MAX_ATTEMPTS: i32 = BACKOFF_SECONDS.len() as i32;

/// Enqueue an outbox event using an `&mut Transaction<Postgres>`.
pub async fn enqueue_event(
    tx: &mut Transaction<'_, Postgres>,
    business_id: Uuid,
    event_type: &str,
    payload: Value,
) -> Result<(), sqlx::Error> {
    enqueue_event_with_tx(tx.as_mut(), business_id, event_type, payload).await
}

/// Same, but takes the raw connection (lets us call from inside a tx that's
/// been re-borrowed).
pub async fn enqueue_event_with_tx(
    conn: &mut PgConnection,
    business_id: Uuid,
    event_type: &str,
    payload: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO outbox_events (id, business_id, event_type, payload)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(Uuid::new_v4())
    .bind(business_id)
    .bind(event_type)
    .bind(payload)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn run(pool: PgPool, interval: Duration) {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest");
    loop {
        if let Err(e) = tick(&pool, &http).await {
            tracing::error!(err = %e, "dispatcher tick error");
        }
        tokio::time::sleep(interval).await;
    }
}

async fn tick(pool: &PgPool, http: &reqwest::Client) -> anyhow::Result<()> {
    fanout(pool).await?;
    deliver(pool, http).await?;
    Ok(())
}

async fn fanout(pool: &PgPool) -> anyhow::Result<()> {
    // Grab events ready to dispatch in chronological order.
    let events: Vec<(Uuid, Uuid, String, Value)> = sqlx::query_as(
        r#"SELECT id, business_id, event_type, payload
           FROM outbox_events
           WHERE dispatched_at IS NULL
           ORDER BY created_at ASC
           LIMIT 100"#,
    )
    .fetch_all(pool)
    .await?;

    for (event_id, business_id, _event_type, _payload) in events {
        let mut tx = pool.begin().await?;

        // Take a row lock on the event so two workers don't dual-fan-out.
        let still_pending: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM outbox_events
             WHERE id = $1 AND dispatched_at IS NULL FOR UPDATE",
        )
        .bind(event_id)
        .fetch_optional(&mut *tx)
        .await?;
        if still_pending.is_none() {
            tx.rollback().await.ok();
            continue;
        }

        let endpoints: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM webhook_endpoints
             WHERE business_id = $1 AND active = TRUE",
        )
        .bind(business_id)
        .fetch_all(&mut *tx)
        .await?;

        for (endpoint_id,) in endpoints {
            sqlx::query(
                r#"INSERT INTO webhook_deliveries
                    (id, outbox_event_id, webhook_endpoint_id, attempt_count,
                     next_attempt_at, status)
                   VALUES ($1, $2, $3, 0, now(), 'pending')"#,
            )
            .bind(Uuid::new_v4())
            .bind(event_id)
            .bind(endpoint_id)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query("UPDATE outbox_events SET dispatched_at = now() WHERE id = $1")
            .bind(event_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
    }
    Ok(())
}

async fn deliver(pool: &PgPool, http: &reqwest::Client) -> anyhow::Result<()> {
    // Pick deliveries that are due.
    let deliveries: Vec<(Uuid, Uuid, Uuid, i32, DateTime<Utc>)> = sqlx::query_as(
        r#"SELECT id, outbox_event_id, webhook_endpoint_id, attempt_count, next_attempt_at
           FROM webhook_deliveries
           WHERE status = 'pending' AND next_attempt_at <= now()
           ORDER BY next_attempt_at ASC
           LIMIT 50"#,
    )
    .fetch_all(pool)
    .await?;

    for (delivery_id, event_id, endpoint_id, attempt_count, _next) in deliveries {
        // Bump attempt count + push next_attempt_at far enough that a second
        // worker won't pick this row up while we're calling out.
        let advance = sqlx::query(
            r#"UPDATE webhook_deliveries
               SET attempt_count = attempt_count + 1,
                   next_attempt_at = now() + interval '60 seconds'
               WHERE id = $1 AND status = 'pending' AND attempt_count = $2"#,
        )
        .bind(delivery_id)
        .bind(attempt_count)
        .execute(pool)
        .await?;
        if advance.rows_affected() == 0 {
            continue; // someone else got it
        }

        let endpoint: Option<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT url, signing_secret FROM webhook_endpoints WHERE id = $1",
        )
        .bind(endpoint_id)
        .fetch_optional(pool)
        .await?;
        let (url, secret) = match endpoint {
            Some(e) => e,
            None => {
                sqlx::query("UPDATE webhook_deliveries SET status = 'failed' WHERE id = $1")
                    .bind(delivery_id)
                    .execute(pool)
                    .await?;
                continue;
            }
        };

        let event: Option<(String, Value, DateTime<Utc>)> = sqlx::query_as(
            "SELECT event_type, payload, created_at FROM outbox_events WHERE id = $1",
        )
        .bind(event_id)
        .fetch_optional(pool)
        .await?;
        let (event_type, payload, created_at) = match event {
            Some(e) => e,
            None => {
                tracing::error!(%delivery_id, "outbox event vanished");
                continue;
            }
        };

        let body = serde_json::json!({
            "id": event_id,
            "type": event_type,
            "created_at": created_at,
            "data": payload,
        });
        let body_bytes = serde_json::to_vec(&body)?;
        let ts = Utc::now().timestamp();
        let sig = sign(&secret, ts, &body_bytes);

        let result = http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Dodo-Signature", &sig)
            .body(body_bytes)
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                sqlx::query(
                    r#"UPDATE webhook_deliveries
                       SET status = 'delivered', delivered_at = now(),
                           last_status_code = $1, last_error = NULL
                       WHERE id = $2"#,
                )
                .bind(resp.status().as_u16() as i32)
                .bind(delivery_id)
                .execute(pool)
                .await?;
                tracing::info!(%delivery_id, event = %event_type, "webhook delivered");
            }
            Ok(resp) => {
                schedule_retry(
                    pool,
                    delivery_id,
                    attempt_count + 1,
                    Some(resp.status().as_u16() as i32),
                    None,
                )
                .await
                .ok();
            }
            Err(e) => {
                let msg = format!("{e}");
                schedule_retry(pool, delivery_id, attempt_count + 1, None, Some(msg))
                    .await
                    .ok();
            }
        };
    }
    Ok(())
}

async fn schedule_retry(
    pool: &PgPool,
    delivery_id: Uuid,
    next_attempt_count: i32,
    status_code: Option<i32>,
    error: Option<String>,
) -> anyhow::Result<()> {
    if next_attempt_count >= MAX_ATTEMPTS {
        sqlx::query(
            r#"UPDATE webhook_deliveries
               SET status = 'failed',
                   last_status_code = $1, last_error = $2
               WHERE id = $3"#,
        )
        .bind(status_code)
        .bind(error.as_deref())
        .bind(delivery_id)
        .execute(pool)
        .await?;
        tracing::warn!(%delivery_id, "webhook delivery exhausted retries");
        return Ok(());
    }

    let secs = BACKOFF_SECONDS[next_attempt_count.min(MAX_ATTEMPTS - 1) as usize];
    let next_at = Utc::now() + ChronoDuration::seconds(secs);

    sqlx::query(
        r#"UPDATE webhook_deliveries
           SET next_attempt_at = $1, last_status_code = $2, last_error = $3
           WHERE id = $4"#,
    )
    .bind(next_at)
    .bind(status_code)
    .bind(error.as_deref())
    .bind(delivery_id)
    .execute(pool)
    .await?;
    Ok(())
}
