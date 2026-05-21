pub mod bootstrap;
pub mod customers;
pub mod invoices;
pub mod payments;
pub mod webhook_endpoints;

use actix_web::HttpResponse;

pub async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({"status": "ok"}))
}
