//! Idempotency: same key + same body - cached response, exactly one PSP call.
//! Same key + different body - 409 idempotency_key_conflict.

mod common;
use common::TestApp;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_key_returns_same_response_and_calls_psp_once() {
    let app = TestApp::spawn().await;
    let customer_id = app.create_customer().await;
    let invoice_id = app.create_invoice(customer_id, 1_000).await;

    let key = "idem-test-key";

    // First call.
    let r1 = app.pay(invoice_id, key, "tok_success").await;
    assert_eq!(r1.status(), 200);
    let b1: serde_json::Value = r1.json().await.unwrap();
    let id1 = b1["id"].as_str().unwrap().to_string();
    let psp_ref1 = b1["psp_ref"].as_str().unwrap().to_string();
    assert_eq!(b1["status"], "succeeded");

    // Repeated 9× with the same key - must return the identical attempt and
    // identical psp_ref. Same psp_ref means exactly one PSP call happened
    // (a second PSP call would generate a new psp_ref).
    for _ in 0..9 {
        let r = app.pay(invoice_id, key, "tok_success").await;
        let status = r.status();
        let b: serde_json::Value = r.json().await.unwrap();
        assert_eq!(status, 200, "replay must be 200, got {status} {b:?}");
        assert_eq!(b["id"].as_str().unwrap(), id1, "same attempt id");
        assert_eq!(
            b["psp_ref"].as_str().unwrap(),
            psp_ref1,
            "same psp_ref - one PSP call"
        );
        assert_eq!(b["status"], "succeeded");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_key_with_different_body_is_conflict() {
    let app = TestApp::spawn().await;
    let customer_id = app.create_customer().await;
    let invoice_id = app.create_invoice(customer_id, 1_000).await;

    let key = "idem-conflict-key";
    let r1 = app.pay(invoice_id, key, "tok_success").await;
    assert_eq!(r1.status(), 200);

    // Different card_token = different request body.
    let r2 = app.pay(invoice_id, key, "tok_card_declined").await;
    assert_eq!(r2.status(), 409);
    let b: serde_json::Value = r2.json().await.unwrap();
    assert_eq!(b["error"]["type"], "idempotency_key_conflict");
}
