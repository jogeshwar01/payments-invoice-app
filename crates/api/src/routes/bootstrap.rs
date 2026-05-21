//! Bootstrap endpoint: create a business and return its first API key in plaintext.
//!
//! This route is NO-AUTH on purpose - it's the entry point for the demo/curl
//! flow. In a real product this would be a console-only operation. Documented
//! as such in the README.

use crate::auth::{display_prefix, generate_key, hash_key};
use crate::error::ApiError;
use crate::AppState;
use actix_web::{web, HttpResponse, Scope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn scope() -> Scope {
    web::scope("/businesses")
        .route("", web::post().to(create_business))
        .route("/{id}/api_keys", web::post().to(create_api_key))
}

#[derive(Deserialize)]
struct CreateBusinessReq {
    name: String,
}

#[derive(Serialize)]
struct CreateBusinessRes {
    id: Uuid,
    name: String,
    api_key: String,
    api_key_prefix: String,
}

async fn create_business(
    state: web::Data<AppState>,
    body: web::Json<CreateBusinessReq>,
) -> Result<HttpResponse, ApiError> {
    let req = body.into_inner();
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }

    let mut tx = state.db.begin().await?;
    let business_id = Uuid::new_v4();

    sqlx::query("INSERT INTO businesses (id, name) VALUES ($1, $2)")
        .bind(business_id)
        .bind(&req.name)
        .execute(&mut *tx)
        .await?;

    let plaintext = generate_key();
    let prefix = display_prefix(&plaintext);
    let hash = hash_key(&plaintext);

    sqlx::query(
        r#"INSERT INTO api_keys (id, business_id, key_hash, key_prefix, name)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(Uuid::new_v4())
    .bind(business_id)
    .bind(&hash)
    .bind(&prefix)
    .bind("default")
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    tracing::info!(%business_id, "business created");
    Ok(HttpResponse::Created().json(CreateBusinessRes {
        id: business_id,
        name: req.name,
        api_key: plaintext,
        api_key_prefix: prefix,
    }))
}

#[derive(Deserialize)]
struct CreateKeyReq {
    name: Option<String>,
}

#[derive(Serialize)]
struct CreateKeyRes {
    id: Uuid,
    api_key: String,
    api_key_prefix: String,
}

async fn create_api_key(
    state: web::Data<AppState>,
    path: web::Path<Uuid>,
    body: web::Json<CreateKeyReq>,
) -> Result<HttpResponse, ApiError> {
    let business_id = path.into_inner();
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM businesses WHERE id = $1")
        .bind(business_id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound("business not found".into()));
    }

    let plaintext = generate_key();
    let prefix = display_prefix(&plaintext);
    let hash = hash_key(&plaintext);
    let key_id = Uuid::new_v4();

    sqlx::query(
        r#"INSERT INTO api_keys (id, business_id, key_hash, key_prefix, name)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(key_id)
    .bind(business_id)
    .bind(&hash)
    .bind(&prefix)
    .bind(body.name.clone().unwrap_or_default())
    .execute(&state.db)
    .await?;

    Ok(HttpResponse::Created().json(CreateKeyRes {
        id: key_id,
        api_key: plaintext,
        api_key_prefix: prefix,
    }))
}
