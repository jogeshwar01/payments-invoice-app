use crate::auth::BusinessCtx;
use crate::error::ApiError;
use crate::AppState;
use actix_web::{web, HttpResponse, Scope};
use base64::Engine;
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn scope() -> Scope {
    web::scope("/webhook_endpoints")
        .route("", web::post().to(create))
        .route("", web::get().to(list))
        .route("/{id}", web::delete().to(delete))
}

#[derive(Deserialize)]
struct CreateReq {
    url: String,
}

#[derive(Serialize)]
struct CreateRes {
    id: Uuid,
    url: String,
    signing_secret: String,
    active: bool,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct WebhookEndpoint {
    id: Uuid,
    url: String,
    signing_secret_prefix: String,
    active: bool,
    created_at: DateTime<Utc>,
}

async fn create(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    body: web::Json<CreateReq>,
) -> Result<HttpResponse, ApiError> {
    let req = body.into_inner();
    if !(req.url.starts_with("http://") || req.url.starts_with("https://")) {
        return Err(ApiError::BadRequest("url must be http(s)".into()));
    }

    let mut secret = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret);
    let id = Uuid::new_v4();

    let row: (DateTime<Utc>,) = sqlx::query_as(
        r#"INSERT INTO webhook_endpoints (id, business_id, url, signing_secret, active)
           VALUES ($1, $2, $3, $4, TRUE)
           RETURNING created_at"#,
    )
    .bind(id)
    .bind(ctx.business_id)
    .bind(&req.url)
    .bind(&secret[..])
    .fetch_one(&state.db)
    .await?;

    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret);
    Ok(HttpResponse::Created().json(CreateRes {
        id,
        url: req.url,
        signing_secret: format!("whsec_{encoded}"),
        active: true,
        created_at: row.0,
    }))
}

async fn list(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
) -> Result<HttpResponse, ApiError> {
    let rows: Vec<(Uuid, String, Vec<u8>, bool, DateTime<Utc>)> = sqlx::query_as(
        r#"SELECT id, url, signing_secret, active, created_at
           FROM webhook_endpoints
           WHERE business_id = $1
           ORDER BY created_at DESC"#,
    )
    .bind(ctx.business_id)
    .fetch_all(&state.db)
    .await?;

    let data: Vec<WebhookEndpoint> = rows
        .into_iter()
        .map(|(id, url, secret, active, created_at)| {
            let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&secret[..4]);
            WebhookEndpoint {
                id,
                url,
                signing_secret_prefix: format!("whsec_{encoded}..."),
                active,
                created_at,
            }
        })
        .collect();
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": data })))
}

async fn delete(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let res = sqlx::query(
        "DELETE FROM webhook_endpoints WHERE id = $1 AND business_id = $2",
    )
    .bind(path.into_inner())
    .bind(ctx.business_id)
    .execute(&state.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound("webhook endpoint not found".into()));
    }
    Ok(HttpResponse::NoContent().finish())
}
