use crate::auth::BusinessCtx;
use crate::error::ApiError;
use crate::AppState;
use actix_web::{web, HttpResponse, Scope};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

pub fn scope() -> Scope {
    web::scope("/customers")
        .route("", web::post().to(create))
        .route("", web::get().to(list))
        .route("/{id}", web::get().to(get_one))
}

#[derive(Deserialize)]
struct CreateReq {
    name: String,
    email: String,
}

#[derive(Serialize, FromRow)]
pub struct Customer {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

async fn create(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    body: web::Json<CreateReq>,
) -> Result<HttpResponse, ApiError> {
    let req = body.into_inner();
    if req.name.trim().is_empty() || req.email.trim().is_empty() {
        return Err(ApiError::BadRequest("name and email are required".into()));
    }

    let id = Uuid::new_v4();
    let rec: Customer = sqlx::query_as(
        r#"INSERT INTO customers (id, business_id, name, email)
           VALUES ($1, $2, $3, $4)
           RETURNING id, name, email, created_at"#,
    )
    .bind(id)
    .bind(ctx.business_id)
    .bind(&req.name)
    .bind(&req.email)
    .fetch_one(&state.db)
    .await?;

    Ok(HttpResponse::Created().json(rec))
}

async fn list(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
) -> Result<HttpResponse, ApiError> {
    let rows: Vec<Customer> = sqlx::query_as(
        r#"SELECT id, name, email, created_at
           FROM customers WHERE business_id = $1
           ORDER BY created_at DESC
           LIMIT 100"#,
    )
    .bind(ctx.business_id)
    .fetch_all(&state.db)
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": rows })))
}

async fn get_one(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let rec: Option<Customer> = sqlx::query_as(
        r#"SELECT id, name, email, created_at
           FROM customers WHERE id = $1 AND business_id = $2"#,
    )
    .bind(id)
    .bind(ctx.business_id)
    .fetch_optional(&state.db)
    .await?;

    let rec = rec.ok_or_else(|| ApiError::NotFound("customer not found".into()))?;
    Ok(HttpResponse::Ok().json(rec))
}
