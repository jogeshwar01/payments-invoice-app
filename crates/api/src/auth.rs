//! API key authentication.
//!
//! Keys are formatted as `dodo_sk_live_<base64url(32 bytes)>`. We store only
//! the SHA-256 hash plus a short prefix for dashboard display. Lookup at auth
//! time is a single indexed query on `key_hash`.

use crate::error::ApiError;
use crate::AppState;
use actix_web::dev::Payload;
use actix_web::{web, FromRequest, HttpRequest};
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::future::{ready, Ready};
use uuid::Uuid;

pub const KEY_PREFIX: &str = "dodo_sk_live_";

#[derive(Clone, Debug)]
pub struct BusinessCtx {
    pub business_id: Uuid,
}

pub fn hash_key(plaintext: &str) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(plaintext.as_bytes());
    h.finalize().to_vec()
}

pub fn generate_key() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!("{KEY_PREFIX}{encoded}")
}

/// Returns the first 8 chars of the random suffix, for display purposes.
pub fn display_prefix(plaintext: &str) -> String {
    let suffix = plaintext.strip_prefix(KEY_PREFIX).unwrap_or(plaintext);
    let take = suffix.chars().take(8).collect::<String>();
    format!("{KEY_PREFIX}{take}")
}

async fn lookup_business(
    state: &AppState,
    plaintext: &str,
) -> Result<Uuid, ApiError> {
    if !plaintext.starts_with(KEY_PREFIX) {
        return Err(ApiError::Unauthorized("invalid api key".into()));
    }
    let hash = hash_key(plaintext);
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT business_id FROM api_keys WHERE key_hash = $1 AND revoked_at IS NULL",
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await
    .map_err(ApiError::from)?;

    match row {
        Some((id,)) => Ok(id),
        None => Err(ApiError::Unauthorized("invalid api key".into())),
    }
}

impl FromRequest for BusinessCtx {
    type Error = ApiError;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self, ApiError>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let header = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());
        let state = req.app_data::<web::Data<AppState>>().cloned();

        Box::pin(async move {
            let state = state
                .ok_or_else(|| ApiError::Internal("missing app state".into()))?;
            let header = header
                .ok_or_else(|| ApiError::Unauthorized("missing authorization header".into()))?;
            let token = header
                .strip_prefix("Bearer ")
                .or_else(|| header.strip_prefix("bearer "))
                .ok_or_else(|| ApiError::Unauthorized("expected bearer token".into()))?;

            let business_id = lookup_business(state.get_ref(), token).await?;
            Ok(BusinessCtx { business_id })
        })
    }
}

/// FromRequest extractor for the `Idempotency-Key` header, required on POST /pay.
pub struct IdempotencyKey(pub String);

impl FromRequest for IdempotencyKey {
    type Error = ApiError;
    type Future = Ready<Result<Self, ApiError>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let v = req
            .headers()
            .get("idempotency-key")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_owned());
        ready(match v {
            Some(s) if !s.is_empty() && s.len() <= 256 => Ok(IdempotencyKey(s)),
            Some(_) => Err(ApiError::BadRequest(
                "Idempotency-Key must be 1..=256 chars".into(),
            )),
            None => Err(ApiError::BadRequest(
                "Idempotency-Key header is required".into(),
            )),
        })
    }
}
