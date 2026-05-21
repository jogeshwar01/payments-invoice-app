//! Consistent JSON error format across the whole API.
//!
//! ```json
//! { "error": { "type": "...", "message": "...", "request_id": "..." } }
//! ```

use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub r#type: String,
    pub message: String,
    pub request_id: String,
}

#[derive(Debug)]
pub enum ApiError {
    /// 400 Bad Request - malformed input
    BadRequest(String),
    /// 401 Unauthorized - missing/invalid API key
    Unauthorized(String),
    /// 404 Not Found
    NotFound(String),
    /// 409 Conflict - idempotency key reuse with different body, or payment in flight
    Conflict { kind: &'static str, message: String },
    /// 422 Unprocessable Entity - invalid state transition or domain rule violation
    Unprocessable { kind: &'static str, message: String },
    /// 500 Internal Server Error
    Internal(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::BadRequest(m) => write!(f, "bad_request: {m}"),
            ApiError::Unauthorized(m) => write!(f, "unauthorized: {m}"),
            ApiError::NotFound(m) => write!(f, "not_found: {m}"),
            ApiError::Conflict { kind, message } => write!(f, "{kind}: {message}"),
            ApiError::Unprocessable { kind, message } => write!(f, "{kind}: {message}"),
            ApiError::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    fn type_str(&self) -> &str {
        match self {
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Unauthorized(_) => "unauthorized",
            ApiError::NotFound(_) => "not_found",
            ApiError::Conflict { kind, .. } => kind,
            ApiError::Unprocessable { kind, .. } => kind,
            ApiError::Internal(_) => "internal_error",
        }
    }

    fn message(&self) -> String {
        match self {
            ApiError::BadRequest(m) => m.clone(),
            ApiError::Unauthorized(m) => m.clone(),
            ApiError::NotFound(m) => m.clone(),
            ApiError::Conflict { message, .. } => message.clone(),
            ApiError::Unprocessable { message, .. } => message.clone(),
            ApiError::Internal(_) => "an internal error occurred".into(),
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict { .. } => StatusCode::CONFLICT,
            ApiError::Unprocessable { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        // We don't have a request-id middleware in the MVP, so synthesize per
        // response. In prod this would come from a tracing middleware.
        let body = serde_json::json!({
            "error": {
                "type": self.type_str(),
                "message": self.message(),
                "request_id": Uuid::new_v4().to_string(),
            }
        });

        if let ApiError::Internal(detail) = self {
            tracing::error!(error = %detail, "internal error");
        }

        HttpResponse::build(self.status_code()).json(body)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => ApiError::NotFound("resource not found".into()),
            other => ApiError::Internal(format!("db error: {other}")),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}
