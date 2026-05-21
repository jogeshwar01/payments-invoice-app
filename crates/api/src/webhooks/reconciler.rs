//! Pending-attempt reconciler.
//!
//! Scans payment_attempts with status='pending' older than the stale window.
//! Looks each one up on the PSP (using our attempt_id as the PSP idempotency
//! key) and finalizes Tx B if the PSP returns an outcome.
//!
//! This is the recovery mechanism for:
//!   - tok_timeout (PSP eventually succeeds; the foreground request returned
//!     202 after the timeout).
//!   - Crash between PSP response and Tx B commit (PSP knows the outcome,
//!     our DB doesn't yet).
//!   - Network errors where we don't know whether the PSP saw the request.
//!
//! Because the PSP returns its stored outcome for known idempotency keys, we
//! never re-issue a charge. Customer is charged at most once.

use crate::psp_client::PspOutcome;
use crate::routes::payments::{finalize_tx_b, TxBOutcome};
use crate::AppState;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::time::Duration;
use uuid::Uuid;

pub async fn run(state: AppState, interval: Duration, stale_after: Duration) {
    loop {
        if let Err(e) = tick(&state, stale_after).await {
            tracing::error!(err = %e, "reconciler tick error");
        }
        tokio::time::sleep(interval).await;
    }
}

async fn tick(state: &AppState, stale_after: Duration) -> anyhow::Result<()> {
    let cutoff: DateTime<Utc> =
        Utc::now() - ChronoDuration::seconds(stale_after.as_secs() as i64);

    let rows: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT id, invoice_id, business_id
           FROM payment_attempts
           WHERE status = 'pending' AND created_at < $1
           ORDER BY created_at ASC
           LIMIT 50"#,
    )
    .bind(cutoff)
    .fetch_all(&state.db)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }
    tracing::debug!(count = rows.len(), "reconciler: stale pending attempts");

    for (attempt_id, invoice_id, business_id) in rows {
        match state.psp.lookup(attempt_id).await {
            Ok(Some(PspOutcome::Succeeded { psp_ref })) => {
                tracing::info!(%attempt_id, "reconciler: PSP says succeeded");
                if let Err(e) = finalize_tx_b(
                    state,
                    business_id,
                    invoice_id,
                    attempt_id,
                    TxBOutcome::Success { psp_ref },
                )
                .await
                {
                    tracing::error!(%attempt_id, err = ?e, "reconcile finalize failed");
                }
            }
            Ok(Some(PspOutcome::Failed { code })) => {
                tracing::info!(%attempt_id, %code, "reconciler: PSP says failed");
                if let Err(e) = finalize_tx_b(
                    state,
                    business_id,
                    invoice_id,
                    attempt_id,
                    TxBOutcome::Failed { code },
                )
                .await
                {
                    tracing::error!(%attempt_id, err = ?e, "reconcile finalize failed");
                }
            }
            Ok(None) => {
                tracing::debug!(%attempt_id, "reconciler: PSP says not_found yet (will retry)");
            }
            Err(e) => {
                tracing::warn!(%attempt_id, err = %e, "reconciler: PSP lookup error");
            }
        }
    }
    Ok(())
}
