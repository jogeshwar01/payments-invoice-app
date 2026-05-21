//! Mock Payment Service Provider.
//!
//! Behaviour is determined by the `card_token` in the request body, per the
//! assignment spec:
//!
//!   tok_success          -> succeed after ~100 ms
//!   tok_insufficient_funds -> fail after ~100 ms
//!   tok_card_declined    -> fail after ~100 ms
//!   tok_timeout          -> sleep 30 s then succeed
//!   tok_network_error    -> 500 immediately
//!
//! A `GET /psp/charges/{idempotency_key}` endpoint exists so our reconciler can
//! look up the eventual outcome of a charge whose response we missed.

use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use uuid::Uuid;

#[derive(Deserialize, Clone)]
struct ChargeRequest {
    idempotency_key: String,
    amount_cents: i64,
    card_token: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ChargeOutcome {
    Succeeded { psp_ref: String },
    Failed { code: String },
}

#[derive(Default)]
struct State {
    // idempotency_key -> outcome
    charges: Mutex<HashMap<String, ChargeOutcome>>,
    // For tests: how many real charge attempts did we process?
    call_count: Mutex<u64>,
}

async fn create_charge(
    state: web::Data<State>,
    body: web::Json<ChargeRequest>,
) -> impl Responder {
    let req = body.into_inner();
    tracing::info!(
        idem = %req.idempotency_key,
        amount = req.amount_cents,
        token = %req.card_token,
        "psp: charge requested"
    );

    // Idempotency on the PSP side: if we've seen this key, replay.
    if let Some(existing) = state.charges.lock().unwrap().get(&req.idempotency_key).cloned() {
        tracing::info!(idem = %req.idempotency_key, "psp: replaying existing outcome");
        return HttpResponse::Ok().json(existing);
    }

    *state.call_count.lock().unwrap() += 1;

    let outcome = match req.card_token.as_str() {
        "tok_success" => {
            tokio::time::sleep(Duration::from_millis(100)).await;
            ChargeOutcome::Succeeded { psp_ref: Uuid::new_v4().to_string() }
        }
        "tok_insufficient_funds" => {
            tokio::time::sleep(Duration::from_millis(100)).await;
            ChargeOutcome::Failed { code: "insufficient_funds".into() }
        }
        "tok_card_declined" => {
            tokio::time::sleep(Duration::from_millis(100)).await;
            ChargeOutcome::Failed { code: "card_declined".into() }
        }
        "tok_timeout" => {
            // The spec: sleeps 30s then returns success. We persist the eventual
            // success so the reconciler's GET-lookup finds it.
            tokio::time::sleep(Duration::from_secs(30)).await;
            ChargeOutcome::Succeeded { psp_ref: Uuid::new_v4().to_string() }
        }
        "tok_network_error" => {
            tokio::time::sleep(Duration::from_millis(50)).await;
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "simulated_network_error"}));
        }
        other => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "unknown_token",
                "token": other
            }));
        }
    };

    state.charges.lock().unwrap().insert(req.idempotency_key.clone(), outcome.clone());
    HttpResponse::Ok().json(outcome)
}

async fn lookup_charge(
    state: web::Data<State>,
    path: web::Path<String>,
) -> impl Responder {
    let key = path.into_inner();
    match state.charges.lock().unwrap().get(&key) {
        Some(o) => HttpResponse::Ok().json(o.clone()),
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "not_found"})),
    }
}

async fn call_count(state: web::Data<State>) -> impl Responder {
    let n = *state.call_count.lock().unwrap();
    HttpResponse::Ok().json(serde_json::json!({"call_count": n}))
}

async fn reset_state(state: web::Data<State>) -> impl Responder {
    state.charges.lock().unwrap().clear();
    *state.call_count.lock().unwrap() = 0;
    HttpResponse::NoContent().finish()
}

async fn health() -> impl Responder {
    HttpResponse::Ok().body("ok")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let bind = std::env::var("MOCK_PSP_BIND").unwrap_or_else(|_| "0.0.0.0:8081".into());
    let state = web::Data::new(State::default());
    tracing::info!(%bind, "mock-psp starting");

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .app_data(web::JsonConfig::default().limit(64 * 1024))
            .route("/health", web::get().to(health))
            .route("/psp/charges", web::post().to(create_charge))
            .route("/psp/charges/{idempotency_key}", web::get().to(lookup_charge))
            .route("/psp/_debug/call_count", web::get().to(call_count))
            .route("/psp/_debug/reset", web::post().to(reset_state))
    })
    .bind(bind)?
    .run()
    .await
}
