//! Concurrency: 20 concurrent POST /pay for the same invoice (distinct
//! idempotency keys) must produce exactly one successful charge.

mod common;
use common::TestApp;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_pay_charges_once() {
    let app = TestApp::spawn().await;
    let customer_id = app.create_customer().await;
    let invoice_id = app.create_invoice(customer_id, 12_300).await;

    let mut handles = Vec::new();
    for i in 0..20 {
        let app2 = TestApp {
            base_url: app.base_url.clone(),
            psp_base_url: app.psp_base_url.clone(),
            http: app.http.clone(),
            api_key: app.api_key.clone(),
            business_id: app.business_id,
        };
        let h = tokio::spawn(async move {
            let key = format!("concurrent-{i}");
            app2.pay(invoice_id, &key, "tok_success").await
        });
        handles.push(h);
    }

    let mut succeeded = 0;
    let mut conflict = 0;
    let mut other = Vec::new();
    for h in handles {
        let resp = h.await.unwrap();
        let status = resp.status().as_u16();
        let body: serde_json::Value = resp.json().await.unwrap();
        match status {
            200 => {
                if body["status"] == "succeeded" {
                    succeeded += 1;
                } else {
                    other.push(format!("200 {body:?}"));
                }
            }
            409 => conflict += 1,
            _ => other.push(format!("{status} {body:?}")),
        }
    }

    assert_eq!(succeeded, 1, "expected exactly 1 success, got {succeeded}");
    assert!(conflict >= 1, "expected at least 1 conflict");
    assert!(succeeded + conflict + other.len() == 20, "all responses accounted: {other:?}");

    let invoice = app.get_invoice(invoice_id).await;
    assert_eq!(invoice["state"], "paid", "final state must be paid");
}
