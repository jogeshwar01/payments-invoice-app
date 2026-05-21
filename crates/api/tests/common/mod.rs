//! Shared test harness: spins up an in-process API server bound to a random
//! port, against a real Postgres (assumed running on DATABASE_URL) and a real
//! mock PSP (assumed running on PSP_BASE_URL). Each test creates its own
//! business so data is isolated by business_id.
//!
//! Run `docker compose up -d postgres mock-psp` before invoking these tests.

#![allow(dead_code)]

use dodo_api::{config::Config, db, psp_client::PspClient, webhooks, AppState};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

pub struct TestApp {
    pub base_url: String,
    pub psp_base_url: String,
    pub http: reqwest::Client,
    pub api_key: String,
    pub business_id: uuid::Uuid,
}

impl TestApp {
    pub async fn spawn() -> Self {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| "postgres://dodo:dodo@localhost:7000/dodo".into());
        let psp_base_url = std::env::var("TEST_PSP_BASE_URL")
            .or_else(|_| std::env::var("PSP_BASE_URL"))
            .unwrap_or_else(|_| "http://localhost:7001".into());

        let pool = db::connect(&database_url).await.expect("connect db");
        db::migrate(&pool).await.expect("migrate");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let bind = format!("127.0.0.1:{port}");

        // Aggressive intervals so tests don't wait long.
        let config = Config {
            database_url: database_url.clone(),
            bind: bind.clone(),
            psp_base_url: psp_base_url.clone(),
            psp_timeout: Duration::from_secs(5),
            reconciler_interval: Duration::from_millis(500),
            reconciler_stale_after: Duration::from_millis(500),
            dispatcher_interval: Duration::from_millis(500),
        };

        let psp = Arc::new(PspClient::new(psp_base_url.clone(), config.psp_timeout));
        let state = AppState {
            db: pool.clone(),
            psp,
            config: Arc::new(config.clone()),
        };

        // Background workers.
        let dispatcher_pool = pool.clone();
        let dispatcher_interval = config.dispatcher_interval;
        tokio::spawn(async move {
            webhooks::dispatcher::run(dispatcher_pool, dispatcher_interval).await;
        });
        let reconciler_state = state.clone();
        let reconciler_interval = config.reconciler_interval;
        let reconciler_stale_after = config.reconciler_stale_after;
        tokio::spawn(async move {
            webhooks::reconciler::run(reconciler_state, reconciler_interval, reconciler_stale_after).await;
        });

        // HTTP server.
        let state_for_factory = state.clone();
        let server = actix_web::HttpServer::new(move || dodo_api::build_app(state_for_factory.clone()))
            .bind(&bind)
            .expect("bind http")
            .run();
        let handle = server.handle();
        tokio::spawn(async move {
            let _ = server.await;
        });
        // Hand off so the server is ready before we return.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = handle; // keep alive

        let base_url = format!("http://{bind}");
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();

        // Reset the PSP between tests if available.
        let _ = http
            .post(format!("{psp_base_url}/psp/_debug/reset"))
            .send()
            .await;

        // Bootstrap a fresh business for this test.
        let resp = http
            .post(format!("{base_url}/v1/businesses"))
            .json(&serde_json::json!({"name": "Test Co"}))
            .send()
            .await
            .expect("bootstrap");
        assert_eq!(resp.status(), 201, "bootstrap failed");
        let body: serde_json::Value = resp.json().await.unwrap();

        TestApp {
            base_url,
            psp_base_url,
            http,
            api_key: body["api_key"].as_str().unwrap().to_string(),
            business_id: body["id"].as_str().unwrap().parse().unwrap(),
        }
    }

    pub async fn psp_call_count(&self) -> u64 {
        let r: serde_json::Value = self
            .http
            .get(format!("{}/psp/_debug/call_count", self.psp_base_url))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        r["call_count"].as_u64().unwrap()
    }

    pub async fn create_customer(&self) -> uuid::Uuid {
        let resp = self
            .http
            .post(format!("{}/v1/customers", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({"name": "Test Customer", "email": "t@example.com"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let body: serde_json::Value = resp.json().await.unwrap();
        body["id"].as_str().unwrap().parse().unwrap()
    }

    /// Create an invoice in open state with the given total in cents.
    pub async fn create_invoice(&self, customer_id: uuid::Uuid, total_cents: i64) -> uuid::Uuid {
        let resp = self
            .http
            .post(format!(
                "{}/v1/invoices?finalize=true",
                self.base_url
            ))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "customer_id": customer_id,
                "line_items": [
                    {"description": "Item", "quantity": 1, "unit_amount_cents": total_cents}
                ]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "create invoice: {}", resp.text().await.unwrap());
        let body: serde_json::Value = resp.json().await.unwrap();
        body["id"].as_str().unwrap().parse().unwrap()
    }

    pub async fn get_invoice(&self, id: uuid::Uuid) -> serde_json::Value {
        let resp = self
            .http
            .get(format!("{}/v1/invoices/{id}", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        resp.json().await.unwrap()
    }

    pub async fn pay(
        &self,
        invoice_id: uuid::Uuid,
        idem_key: &str,
        card_token: &str,
    ) -> reqwest::Response {
        self.http
            .post(format!("{}/v1/invoices/{invoice_id}/pay", self.base_url))
            .bearer_auth(&self.api_key)
            .header("Idempotency-Key", idem_key)
            .json(&serde_json::json!({"card_token": card_token}))
            .send()
            .await
            .unwrap()
    }
}
