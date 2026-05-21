//! HTTP client for the mock PSP.
//!
//! Critical points:
//!  - A short total timeout (default 5s) so the caller of /pay never waits
//!    for the PSP's 30 s tok_timeout behaviour.
//!  - We pass our *own* attempt_id as the PSP's idempotency key. After a
//!    crash, the reconciler can recover the outcome via GET /psp/charges/{id}.
//!  - No client-side retries: idempotency lives at our layer, and retries
//!    here would double-fire a charge under timeout conditions.

use crate::money::Cents;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct PspClient {
    http: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Serialize)]
struct ChargeRequest<'a> {
    idempotency_key: Uuid,
    amount_cents: i64,
    card_token: &'a str,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PspOutcome {
    Succeeded { psp_ref: String },
    Failed { code: String },
}

#[derive(Debug)]
pub enum PspError {
    Timeout,
    NetworkError(String),
    NotFound,
    Unexpected(String),
}

impl std::fmt::Display for PspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PspError::Timeout => write!(f, "psp timeout"),
            PspError::NetworkError(s) => write!(f, "psp network error: {s}"),
            PspError::NotFound => write!(f, "psp charge not found"),
            PspError::Unexpected(s) => write!(f, "psp unexpected: {s}"),
        }
    }
}
impl std::error::Error for PspError {}

impl PspClient {
    pub fn new(base_url: String, timeout: Duration) -> Self {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(2))
            .build()
            .expect("reqwest client");
        Self { http, base_url }
    }

    pub async fn charge(
        &self,
        attempt_id: Uuid,
        amount: Cents,
        card_token: &str,
    ) -> Result<PspOutcome, PspError> {
        let req = ChargeRequest {
            idempotency_key: attempt_id,
            amount_cents: amount.into(),
            card_token,
        };
        let res = self
            .http
            .post(format!("{}/psp/charges", self.base_url))
            .json(&req)
            .send()
            .await;

        match res {
            Ok(resp) => match resp.status() {
                StatusCode::OK => resp
                    .json::<PspOutcome>()
                    .await
                    .map_err(|e| PspError::Unexpected(format!("decode: {e}"))),
                StatusCode::INTERNAL_SERVER_ERROR => {
                    Err(PspError::NetworkError("psp 500".into()))
                }
                s => {
                    let body = resp.text().await.unwrap_or_default();
                    Err(PspError::Unexpected(format!("status {s}: {body}")))
                }
            },
            Err(e) if e.is_timeout() => Err(PspError::Timeout),
            Err(e) if e.is_connect() => Err(PspError::NetworkError(e.to_string())),
            Err(e) => Err(PspError::NetworkError(e.to_string())),
        }
    }

    /// Reconciliation lookup. We key PSP charges by our attempt_id, so after a
    /// crash or timeout the reconciler can recover the eventual outcome.
    pub async fn lookup(&self, attempt_id: Uuid) -> Result<Option<PspOutcome>, PspError> {
        let res = self
            .http
            .get(format!("{}/psp/charges/{}", self.base_url, attempt_id))
            .send()
            .await
            .map_err(|e| PspError::NetworkError(e.to_string()))?;

        match res.status() {
            StatusCode::OK => Ok(Some(res.json().await.map_err(|e| {
                PspError::Unexpected(format!("decode: {e}"))
            })?)),
            StatusCode::NOT_FOUND => Ok(None),
            s => Err(PspError::Unexpected(format!("lookup status {s}"))),
        }
    }
}
