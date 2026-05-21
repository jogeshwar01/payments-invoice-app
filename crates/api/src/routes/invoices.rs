//! Invoice routes.
//!
//! Server computes the total — we never trust a client-supplied total. Line
//! items are validated for non-negative cents and positive quantity, and the
//! sum is checked for overflow.

use crate::auth::BusinessCtx;
use crate::domain::invoice_state::{try_transition, InvoiceState, TransitionEvent};
use crate::error::ApiError;
use crate::money::Cents;
use crate::webhooks::dispatcher::enqueue_event;
use crate::AppState;
use actix_web::{web, HttpResponse, Scope};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn scope() -> Scope {
    web::scope("/invoices")
        .route("", web::post().to(create))
        .route("", web::get().to(list))
        .route("/{id}", web::get().to(get_one))
        .route("/{id}/finalize", web::post().to(finalize))
        .route("/{id}/void", web::post().to(void))
        .route("/{id}/pay", web::post().to(crate::routes::payments::pay_handler))
}

#[derive(Deserialize)]
struct LineItemInput {
    description: String,
    quantity: i32,
    unit_amount_cents: i64,
}

#[derive(Deserialize)]
struct CreateReq {
    customer_id: Uuid,
    line_items: Vec<LineItemInput>,
    due_date: Option<NaiveDate>,
}

#[derive(Deserialize)]
struct CreateQuery {
    #[serde(default)]
    finalize: bool,
}

#[derive(Serialize)]
pub struct LineItem {
    pub id: Uuid,
    pub description: String,
    pub quantity: i32,
    pub unit_amount_cents: i64,
    pub position: i32,
}

#[derive(Serialize)]
pub struct Invoice {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub state: String,
    pub total_cents: i64,
    pub currency: String,
    pub due_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub line_items: Vec<LineItem>,
}

async fn create(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    query: web::Query<CreateQuery>,
    body: web::Json<CreateReq>,
) -> Result<HttpResponse, ApiError> {
    let req = body.into_inner();

    if req.line_items.is_empty() {
        return Err(ApiError::BadRequest("at least one line item required".into()));
    }

    // Server-computed total. We refuse to take a client total.
    let mut total = Cents::ZERO;
    for li in &req.line_items {
        if li.quantity <= 0 {
            return Err(ApiError::BadRequest("quantity must be positive".into()));
        }
        if li.unit_amount_cents < 0 {
            return Err(ApiError::BadRequest("unit_amount_cents must be >= 0".into()));
        }
        if li.description.trim().is_empty() {
            return Err(ApiError::BadRequest("line item description required".into()));
        }
        let line_total = Cents(li.unit_amount_cents)
            .checked_mul_quantity(li.quantity)
            .ok_or_else(|| ApiError::BadRequest("line item overflow".into()))?;
        total = total
            .checked_add(line_total)
            .ok_or_else(|| ApiError::BadRequest("total overflow".into()))?;
    }

    // Verify customer belongs to this business.
    let owner: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM customers WHERE id = $1 AND business_id = $2",
    )
    .bind(req.customer_id)
    .bind(ctx.business_id)
    .fetch_optional(&state.db)
    .await?;
    if owner.is_none() {
        return Err(ApiError::BadRequest("customer not found for business".into()));
    }

    let mut tx = state.db.begin().await?;

    let invoice_id = Uuid::new_v4();
    let initial_state = if query.finalize {
        InvoiceState::Open
    } else {
        InvoiceState::Draft
    };

    sqlx::query(
        r#"INSERT INTO invoices (id, business_id, customer_id, state, total_cents, currency, due_date)
           VALUES ($1, $2, $3, $4, $5, 'USD', $6)"#,
    )
    .bind(invoice_id)
    .bind(ctx.business_id)
    .bind(req.customer_id)
    .bind(initial_state.as_str())
    .bind(i64::from(total))
    .bind(req.due_date)
    .execute(&mut *tx)
    .await?;

    for (idx, li) in req.line_items.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO invoice_line_items
                 (id, invoice_id, description, quantity, unit_amount_cents, position)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(Uuid::new_v4())
        .bind(invoice_id)
        .bind(&li.description)
        .bind(li.quantity)
        .bind(li.unit_amount_cents)
        .bind(idx as i32)
        .execute(&mut *tx)
        .await?;
    }

    // invoice.created event goes to the outbox in the same tx.
    enqueue_event(
        &mut tx,
        ctx.business_id,
        "invoice.created",
        serde_json::json!({
            "invoice_id": invoice_id,
            "customer_id": req.customer_id,
            "state": initial_state.as_str(),
            "total_cents": i64::from(total),
            "currency": "USD",
        }),
    )
    .await?;

    tx.commit().await?;

    let inv = load_invoice(&state, ctx.business_id, invoice_id).await?;
    Ok(HttpResponse::Created().json(inv))
}

#[derive(Deserialize)]
struct ListQuery {
    state: Option<String>,
}

async fn list(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    query: web::Query<ListQuery>,
) -> Result<HttpResponse, ApiError> {
    let rows: Vec<(Uuid, Uuid, String, i64, String, Option<NaiveDate>, DateTime<Utc>, DateTime<Utc>)> =
        if let Some(filter) = &query.state {
            // Validate the filter before issuing the query.
            filter
                .parse::<InvoiceState>()
                .map_err(|e| ApiError::BadRequest(e))?;
            sqlx::query_as(
                r#"SELECT id, customer_id, state, total_cents, currency, due_date, created_at, updated_at
                   FROM invoices
                   WHERE business_id = $1 AND state = $2
                   ORDER BY created_at DESC LIMIT 100"#,
            )
            .bind(ctx.business_id)
            .bind(filter)
            .fetch_all(&state.db)
            .await?
        } else {
            sqlx::query_as(
                r#"SELECT id, customer_id, state, total_cents, currency, due_date, created_at, updated_at
                   FROM invoices
                   WHERE business_id = $1
                   ORDER BY created_at DESC LIMIT 100"#,
            )
            .bind(ctx.business_id)
            .fetch_all(&state.db)
            .await?
        };

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let (id, customer_id, st, total_cents, currency, due_date, created_at, updated_at) = r;
        let line_items = load_line_items(&state, id).await?;
        out.push(Invoice {
            id,
            customer_id,
            state: st,
            total_cents,
            currency,
            due_date,
            created_at,
            updated_at,
            line_items,
        });
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": out })))
}

async fn get_one(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let inv = load_invoice(&state, ctx.business_id, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(inv))
}

async fn finalize(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    apply_simple_transition(&state, ctx.business_id, id, TransitionEvent::Finalize).await?;
    let inv = load_invoice(&state, ctx.business_id, id).await?;
    Ok(HttpResponse::Ok().json(inv))
}

async fn void(
    state: web::Data<AppState>,
    ctx: BusinessCtx,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    apply_simple_transition(&state, ctx.business_id, id, TransitionEvent::Void).await?;
    let inv = load_invoice(&state, ctx.business_id, id).await?;
    Ok(HttpResponse::Ok().json(inv))
}

async fn apply_simple_transition(
    state: &AppState,
    business_id: Uuid,
    invoice_id: Uuid,
    event: TransitionEvent,
) -> Result<(), ApiError> {
    let mut tx = state.db.begin().await?;

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT state FROM invoices WHERE id = $1 AND business_id = $2 FOR UPDATE",
    )
    .bind(invoice_id)
    .bind(business_id)
    .fetch_optional(&mut *tx)
    .await?;
    let cur = row
        .ok_or_else(|| ApiError::NotFound("invoice not found".into()))?
        .0;
    let cur: InvoiceState = cur.parse().map_err(ApiError::Internal)?;

    let next = try_transition(cur, event).map_err(|e| ApiError::Unprocessable {
        kind: "invalid_state_transition",
        message: e.to_string(),
    })?;

    sqlx::query(
        "UPDATE invoices SET state = $1, updated_at = now() WHERE id = $2",
    )
    .bind(next.as_str())
    .bind(invoice_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub(crate) async fn load_invoice(
    state: &AppState,
    business_id: Uuid,
    invoice_id: Uuid,
) -> Result<Invoice, ApiError> {
    let row: Option<(Uuid, Uuid, String, i64, String, Option<NaiveDate>, DateTime<Utc>, DateTime<Utc>)> =
        sqlx::query_as(
            r#"SELECT id, customer_id, state, total_cents, currency, due_date, created_at, updated_at
               FROM invoices WHERE id = $1 AND business_id = $2"#,
        )
        .bind(invoice_id)
        .bind(business_id)
        .fetch_optional(&state.db)
        .await?;

    let (id, customer_id, st, total_cents, currency, due_date, created_at, updated_at) =
        row.ok_or_else(|| ApiError::NotFound("invoice not found".into()))?;

    let line_items = load_line_items(state, id).await?;
    Ok(Invoice {
        id,
        customer_id,
        state: st,
        total_cents,
        currency,
        due_date,
        created_at,
        updated_at,
        line_items,
    })
}

async fn load_line_items(state: &AppState, invoice_id: Uuid) -> Result<Vec<LineItem>, ApiError> {
    let rows: Vec<(Uuid, String, i32, i64, i32)> = sqlx::query_as(
        r#"SELECT id, description, quantity, unit_amount_cents, position
           FROM invoice_line_items WHERE invoice_id = $1 ORDER BY position ASC"#,
    )
    .bind(invoice_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, description, quantity, unit_amount_cents, position)| LineItem {
            id,
            description,
            quantity,
            unit_amount_cents,
            position,
        })
        .collect())
}
