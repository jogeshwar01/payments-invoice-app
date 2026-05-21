//! PSP failure modes:
//!  - tok_timeout: endpoint returns 202 within ~6s (not 30s), reconciler
//!    converges to paid.
//!  - paying an already-paid invoice returns 422.

mod common;
use common::TestApp;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn timeout_does_not_hang_endpoint_and_reconciler_converges() {
    let app = TestApp::spawn().await;
    let customer_id = app.create_customer().await;
    let invoice_id = app.create_invoice(customer_id, 5_000).await;

    let key = "timeout-test";
    let started = Instant::now();
    let resp = app.pay(invoice_id, key, "tok_timeout").await;
    let elapsed = started.elapsed();

    // The endpoint must return well before the PSP's 30s sleep finishes.
    assert!(
        elapsed < Duration::from_secs(10),
        "endpoint took too long: {elapsed:?}"
    );
    assert_eq!(resp.status(), 202, "expected 202 Accepted for timeout");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "pending");

    // Invoice should be in processing state right now.
    let inv = app.get_invoice(invoice_id).await;
    assert_eq!(inv["state"], "processing");

    // Wait for the reconciler to converge. The PSP's tok_timeout actually
    // takes 30 s to return, so give it a generous window.
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let inv = app.get_invoice(invoice_id).await;
        if inv["state"] == "paid" {
            break;
        }
        if Instant::now() > deadline {
            panic!(
                "reconciler did not converge in time; final state: {}",
                inv["state"]
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paid_invoice_rejects_further_pay() {
    let app = TestApp::spawn().await;
    let customer_id = app.create_customer().await;
    let invoice_id = app.create_invoice(customer_id, 5_000).await;

    let r = app.pay(invoice_id, "first", "tok_success").await;
    assert_eq!(r.status(), 200);

    // New idempotency key, paid invoice -> 422.
    let r2 = app.pay(invoice_id, "second", "tok_success").await;
    assert_eq!(r2.status(), 422);
    let b: serde_json::Value = r2.json().await.unwrap();
    assert_eq!(b["error"]["type"], "invalid_state_for_payment");
}
