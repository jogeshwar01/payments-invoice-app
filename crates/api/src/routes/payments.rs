//! POST /v1/invoices/{id}/pay - the correctness-critical endpoint.
//!
//! Flow (two transactions, with the PSP call between them, no lock held over
//! network I/O):
//!
//! ```text
//!   Tx A:
//!     SELECT ... FOR UPDATE on invoices.id              -- row lock
//!     check existing payment_attempts by (business_id, idempotency_key)
//!       -> different request_hash: 409 idempotency_key_conflict
//!       -> same hash, not pending: replay cached response
//!       -> same hash, pending:     return 202 pending
//!     check invoice state == open
//!     INSERT payment_attempts (status=pending)
//!     UPDATE invoices state open -> processing
//!     COMMIT
//!
//!   PSP call (5s client timeout, no retries):
//!     on success/declined: continue to Tx B
//!     on timeout:          return 202 pending; reconciler will finish
//!     on network error:    do Tx B with PspFailure to revert to open
//!
//!   Tx B:
//!     SELECT ... FOR UPDATE on invoices.id              -- row lock
//!     UPDATE payment_attempts with outcome
//!     UPDATE invoices processing -> paid | open
//!     INSERT outbox_events (invoice.paid | invoice.payment_failed)
//!     COMMIT
//! ```
//!
//! Concurrency guarantees:
//!  (a) Two concurrent /pay with different idempotency keys: the SELECT FOR
//!      UPDATE serializes them. First moves invoice to processing, second
//!      wakes up, sees state != open, returns 409 payment_in_progress.
//!  (a') Two concurrent /pay with the same key: UNIQUE(business_id, key) makes
//!      one INSERT fail with 23505. That handler retries the read path and
//!      returns the in-flight pending response.
//!  (b) tok_timeout: endpoint returns 202 within ~5s. Reconciler converges.
//!  (c) Crash between PSP success and Tx B: reconciler looks up by attempt_id
//!      on the PSP and finishes Tx B. Customer charged exactly once.
//!  (d) Key reused with different body: 409 idempotency_key_conflict.
//!  (e) Already-paid invoice + /pay: 422 invalid_state_for_payment.

use crate::auth::{BusinessCtx, IdempotencyKey};
use crate::domain::invoice_state::{try_transition, InvoiceState, TransitionEvent};
use crate::error::ApiError;
use crate::money::Cents;
use crate::psp_client::{PspError, PspOutcome};
use crate::webhooks::dispatcher::enqueue_event_with_tx;
use crate::AppState;
use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn route() -> actix_web::Route {
    web::post().to(pay)
}

pub async fn pay_handler(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    path: web::Path<Uuid>,
    idem: IdempotencyKey,
    body: web::Json<PayRequest>,
) -> Result<HttpResponse, ApiError> {
    pay(state, ctx, path, idem, body).await
}

#[derive(Deserialize, Serialize)]
pub struct PayRequest {
    pub card_token: String,
}

#[derive(Debug, Serialize)]
pub struct PaymentAttemptResponse {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub status: String,
    pub psp_ref: Option<String>,
    pub failure_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

fn hash_request(body: &PayRequest) -> Vec<u8> {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "card_token": body.card_token,
    }))
    .expect("canonicalize");
    let mut h = Sha256::new();
    h.update(&canonical);
    h.finalize().to_vec()
}

#[derive(Debug)]
enum TxAOutcome {
    NewAttempt { attempt_id: Uuid, amount: Cents },
    ReplayCompleted(PaymentAttemptResponse),
    ReplayPending(PaymentAttemptResponse),
    IdempotencyConflict,
    PaymentInProgress,
    InvalidState(String),
    InvoiceNotFound,
}

async fn run_tx_a(
    state: &AppState,
    business_id: Uuid,
    invoice_id: Uuid,
    idem_key: &str,
    request_hash: &[u8],
) -> Result<TxAOutcome, ApiError> {
    let mut tx = state.db.begin().await?;

    // Row lock on the invoice.
    let inv: Option<(String, i64)> = sqlx::query_as(
        "SELECT state, total_cents FROM invoices
         WHERE id = $1 AND business_id = $2 FOR UPDATE",
    )
    .bind(invoice_id)
    .bind(business_id)
    .fetch_optional(&mut *tx)
    .await?;

    let (cur_state, total_cents) = match inv {
        None => return Ok(TxAOutcome::InvoiceNotFound),
        Some(r) => r,
    };

    // Idempotency check. The UNIQUE index makes the (business_id, key) lookup
    // a single index probe.
    let existing: Option<(
        Uuid,
        Vec<u8>,
        String,
        Option<String>,
        Option<String>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        r#"SELECT id, request_hash, status, psp_ref, failure_code, created_at, completed_at
               FROM payment_attempts
               WHERE business_id = $1 AND idempotency_key = $2"#,
    )
    .bind(business_id)
    .bind(idem_key)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some((id, hash, status, psp_ref, failure_code, created_at, completed_at)) = existing {
        if hash != request_hash {
            return Ok(TxAOutcome::IdempotencyConflict);
        }
        let resp = PaymentAttemptResponse {
            id,
            invoice_id,
            status: status.clone(),
            psp_ref,
            failure_code,
            created_at,
            completed_at,
        };
        return Ok(if status == "pending" {
            TxAOutcome::ReplayPending(resp)
        } else {
            TxAOutcome::ReplayCompleted(resp)
        });
    }

    // No existing attempt with this key. State must be 'open' to take a new payment.
    let cur: InvoiceState = cur_state.parse().map_err(ApiError::Internal)?;
    if cur == InvoiceState::Processing {
        return Ok(TxAOutcome::PaymentInProgress);
    }
    if try_transition(cur, TransitionEvent::Pay).is_err() {
        return Ok(TxAOutcome::InvalidState(format!(
            "invoice state is {cur}; cannot pay"
        )));
    }

    let attempt_id = Uuid::new_v4();
    let insert_res = sqlx::query(
        r#"INSERT INTO payment_attempts
            (id, invoice_id, business_id, idempotency_key, request_hash, status, created_at)
           VALUES ($1, $2, $3, $4, $5, 'pending', now())"#,
    )
    .bind(attempt_id)
    .bind(invoice_id)
    .bind(business_id)
    .bind(idem_key)
    .bind(request_hash)
    .execute(&mut *tx)
    .await;

    if let Err(e) = insert_res {
        // 23505 unique_violation: race on the same idempotency key. Re-read
        // and return the in-flight pending response (Tx B handler races us).
        if let Some(dbe) = e.as_database_error() {
            if dbe.code().as_deref() == Some("23505") {
                tx.rollback().await.ok();
                return read_existing(state, business_id, invoice_id, idem_key).await;
            }
        }
        return Err(ApiError::Internal(format!("db: {e}")));
    }

    sqlx::query("UPDATE invoices SET state = 'processing', updated_at = now() WHERE id = $1")
        .bind(invoice_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(TxAOutcome::NewAttempt {
        attempt_id,
        amount: Cents(total_cents),
    })
}

/// Re-read the existing attempt after a 23505 race. Always returns a Replay
/// variant, or InvoiceNotFound if something pathological happened.
async fn read_existing(
    state: &AppState,
    business_id: Uuid,
    invoice_id: Uuid,
    idem_key: &str,
) -> Result<TxAOutcome, ApiError> {
    let row: Option<(
        Uuid,
        String,
        Option<String>,
        Option<String>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        r#"SELECT id, status, psp_ref, failure_code, created_at, completed_at
               FROM payment_attempts
               WHERE business_id = $1 AND idempotency_key = $2"#,
    )
    .bind(business_id)
    .bind(idem_key)
    .fetch_optional(&state.db)
    .await?;

    let (id, status, psp_ref, failure_code, created_at, completed_at) = match row {
        None => return Ok(TxAOutcome::InvoiceNotFound),
        Some(r) => r,
    };

    let resp = PaymentAttemptResponse {
        id,
        invoice_id,
        status: status.clone(),
        psp_ref,
        failure_code,
        created_at,
        completed_at,
    };
    Ok(if status == "pending" {
        TxAOutcome::ReplayPending(resp)
    } else {
        TxAOutcome::ReplayCompleted(resp)
    })
}

async fn pay(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    path: web::Path<Uuid>,
    idem: IdempotencyKey,
    body: web::Json<PayRequest>,
) -> Result<HttpResponse, ApiError> {
    let invoice_id = path.into_inner();
    let req = body.into_inner();
    let request_hash = hash_request(&req);
    let business_id = ctx.business_id;

    let outcome = run_tx_a(&state, business_id, invoice_id, &idem.0, &request_hash).await?;

    let (attempt_id, amount) = match outcome {
        TxAOutcome::NewAttempt { attempt_id, amount } => (attempt_id, amount),
        TxAOutcome::ReplayCompleted(r) => return Ok(HttpResponse::Ok().json(r)),
        TxAOutcome::ReplayPending(r) => return Ok(HttpResponse::Accepted().json(r)),
        TxAOutcome::IdempotencyConflict => {
            return Err(ApiError::Conflict {
                kind: "idempotency_key_conflict",
                message: "idempotency key reused with a different request body".into(),
            })
        }
        TxAOutcome::PaymentInProgress => {
            return Err(ApiError::Conflict {
                kind: "payment_in_progress",
                message: "another payment attempt is in flight on this invoice".into(),
            })
        }
        TxAOutcome::InvalidState(m) => {
            return Err(ApiError::Unprocessable {
                kind: "invalid_state_for_payment",
                message: m,
            })
        }
        TxAOutcome::InvoiceNotFound => return Err(ApiError::NotFound("invoice not found".into())),
    };

    tracing::info!(%invoice_id, %attempt_id, "psp call starting");
    let psp_res = state.psp.charge(attempt_id, amount, &req.card_token).await;

    match psp_res {
        Ok(PspOutcome::Succeeded { psp_ref }) => {
            finalize_tx_b(
                &state,
                business_id,
                invoice_id,
                attempt_id,
                TxBOutcome::Success {
                    psp_ref: psp_ref.clone(),
                },
            )
            .await?;
            let resp = load_attempt(&state, attempt_id, invoice_id).await?;
            Ok(HttpResponse::Ok().json(resp))
        }
        Ok(PspOutcome::Failed { code }) => {
            finalize_tx_b(
                &state,
                business_id,
                invoice_id,
                attempt_id,
                TxBOutcome::Failed { code: code.clone() },
            )
            .await?;
            let resp = load_attempt(&state, attempt_id, invoice_id).await?;
            Ok(HttpResponse::Ok().json(resp))
        }
        Err(PspError::Timeout) => {
            // Don't touch state. Return 202 pending. Reconciler will resolve.
            tracing::warn!(%invoice_id, %attempt_id, "psp timeout - leaving attempt pending");
            let resp = load_attempt(&state, attempt_id, invoice_id).await?;
            Ok(HttpResponse::Accepted().json(resp))
        }
        Err(PspError::NetworkError(e)) => {
            // Network-level error: we don't know if PSP took our request or not.
            // Safest is to leave pending so reconciler can disambiguate via lookup.
            tracing::warn!(%invoice_id, %attempt_id, err = %e, "psp network error - leaving pending");
            let resp = load_attempt(&state, attempt_id, invoice_id).await?;
            Ok(HttpResponse::Accepted().json(resp))
        }
        Err(other) => {
            tracing::error!(%invoice_id, %attempt_id, err = %other, "psp unexpected error");
            let resp = load_attempt(&state, attempt_id, invoice_id).await?;
            Ok(HttpResponse::Accepted().json(resp))
        }
    }
}

#[derive(Debug)]
pub enum TxBOutcome {
    Success { psp_ref: String },
    Failed { code: String },
}

/// Second transaction: writes the outcome and transitions the invoice.
/// Also enqueues the appropriate outbox event in the same tx.
pub(crate) async fn finalize_tx_b(
    state: &AppState,
    business_id: Uuid,
    invoice_id: Uuid,
    attempt_id: Uuid,
    outcome: TxBOutcome,
) -> Result<(), ApiError> {
    let mut tx = state.db.begin().await?;

    let inv: Option<(String,)> =
        sqlx::query_as("SELECT state FROM invoices WHERE id = $1 AND business_id = $2 FOR UPDATE")
            .bind(invoice_id)
            .bind(business_id)
            .fetch_optional(&mut *tx)
            .await?;

    let cur_state = inv
        .ok_or_else(|| ApiError::NotFound("invoice not found".into()))?
        .0;
    let cur: InvoiceState = cur_state.parse().map_err(ApiError::Internal)?;

    // Idempotency on Tx B itself: if the attempt is already final, no-op.
    // This makes the reconciler safe to race with the foreground request.
    let cur_attempt: Option<(String,)> =
        sqlx::query_as("SELECT status FROM payment_attempts WHERE id = $1 FOR UPDATE")
            .bind(attempt_id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((s,)) = cur_attempt {
        if s != "pending" {
            tx.commit().await.ok();
            return Ok(());
        }
    } else {
        return Err(ApiError::Internal("payment_attempt vanished".into()));
    }

    match &outcome {
        TxBOutcome::Success { psp_ref } => {
            // processing -> paid; tolerate already-paid for replay safety.
            if cur == InvoiceState::Processing {
                let next = try_transition(cur, TransitionEvent::PspSuccess)
                    .map_err(|e| ApiError::Internal(format!("unexpected transition: {e}")))?;
                sqlx::query("UPDATE invoices SET state = $1, updated_at = now() WHERE id = $2")
                    .bind(next.as_str())
                    .bind(invoice_id)
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query(
                r#"UPDATE payment_attempts
                   SET status = 'succeeded', psp_ref = $1, completed_at = now(),
                       response_json = $2
                   WHERE id = $3"#,
            )
            .bind(psp_ref)
            .bind(serde_json::json!({"status": "succeeded", "psp_ref": psp_ref}))
            .bind(attempt_id)
            .execute(&mut *tx)
            .await?;

            enqueue_event_with_tx(
                tx.as_mut(),
                business_id,
                "invoice.paid",
                serde_json::json!({
                    "invoice_id": invoice_id,
                    "payment_attempt_id": attempt_id,
                    "psp_ref": psp_ref,
                }),
            )
            .await?;
        }
        TxBOutcome::Failed { code } => {
            if cur == InvoiceState::Processing {
                let next = try_transition(cur, TransitionEvent::PspFailure)
                    .map_err(|e| ApiError::Internal(format!("unexpected transition: {e}")))?;
                sqlx::query("UPDATE invoices SET state = $1, updated_at = now() WHERE id = $2")
                    .bind(next.as_str())
                    .bind(invoice_id)
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query(
                r#"UPDATE payment_attempts
                   SET status = 'failed', failure_code = $1, completed_at = now(),
                       response_json = $2
                   WHERE id = $3"#,
            )
            .bind(code)
            .bind(serde_json::json!({"status": "failed", "code": code}))
            .bind(attempt_id)
            .execute(&mut *tx)
            .await?;

            enqueue_event_with_tx(
                tx.as_mut(),
                business_id,
                "invoice.payment_failed",
                serde_json::json!({
                    "invoice_id": invoice_id,
                    "payment_attempt_id": attempt_id,
                    "failure_code": code,
                }),
            )
            .await?;
        }
    }

    tx.commit().await?;
    Ok(())
}

async fn load_attempt(
    state: &AppState,
    attempt_id: Uuid,
    invoice_id: Uuid,
) -> Result<PaymentAttemptResponse, ApiError> {
    let row: (
        String,
        Option<String>,
        Option<String>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    ) = sqlx::query_as(
        r#"SELECT status, psp_ref, failure_code, created_at, completed_at
               FROM payment_attempts WHERE id = $1"#,
    )
    .bind(attempt_id)
    .fetch_one(&state.db)
    .await?;
    Ok(PaymentAttemptResponse {
        id: attempt_id,
        invoice_id,
        status: row.0,
        psp_ref: row.1,
        failure_code: row.2,
        created_at: row.3,
        completed_at: row.4,
    })
}
