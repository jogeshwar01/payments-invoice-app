{
"id": "c107862c-50b4-4bb8-8840-32540fcc66b0",
"name": "Acme Corp",
"api_key": "dodo_sk_live_KZvmKRmwbtOX8w5uQrY9Ue5mTqLQ8rR6VhjA83QACwM",
"api_key_prefix": "dodo_sk_live_KZvmKRmw"
}

{
"id": "fb02bcbb-deb0-44aa-9de7-22631203e39f",
"name": "Jane Doe",
"email": "jane@example.com",
"created_at": "2026-05-21T10:17:12.167988Z"
}

{
"id": "5b93ebca-ec10-46d7-a80e-715e0def3a4f",
"url": "https://webhook.site/42df54fd-9059-499a-864e-43903e0bec6f",
"signing_secret": "whsec_DYW8BjMEERjNRZgpCrHT816WKEBz6FPjz9VAVC2UrfE",
"active": true,
"created_at": "2026-05-21T10:17:32.250672Z"
}

{
"id": "80c4a742-5a02-4fc6-b5cf-ced661e32f84",
"customer_id": "fb02bcbb-deb0-44aa-9de7-22631203e39f",
"state": "open",
"total_cents": 7300,
"currency": "USD",
"due_date": null,
"created_at": "2026-05-21T10:18:21.555341Z",
"updated_at": "2026-05-21T10:18:21.555341Z",
"line_items": [
{
"id": "400443d1-0e60-488d-8f51-300cd9cc1439",
"description": "Pro plan",
"quantity": 1,
"unit_amount_cents": 4900,
"position": 0
},
{
"id": "5575b98d-9b29-404d-b0cf-87ba7d605999",
"description": "Add-on",
"quantity": 2,
"unit_amount_cents": 1200,
"position": 1
}
]
}

{
"id": "c9b49fa6-6219-4752-9a29-91808de9104f",
"invoice_id": "80c4a742-5a02-4fc6-b5cf-ced661e32f84",
"status": "succeeded",
"psp_ref": "f5227509-370d-4f5c-9d76-b99e4be9ea1d",
"failure_code": null,
"created_at": "2026-05-21T10:19:07.211889Z",
"completed_at": "2026-05-21T10:19:07.323865Z"
}

WEBHOOKS-

{
"created_at": "2026-05-21T10:18:21.555341Z",
"data": {
"currency": "USD",
"customer_id": "fb02bcbb-deb0-44aa-9de7-22631203e39f",
"invoice_id": "80c4a742-5a02-4fc6-b5cf-ced661e32f84",
"state": "open",
"total_cents": 7300
},
"id": "fe8beddc-80e3-492f-8c53-c6dc098464e6",
"type": "invoice.created"
}

{
"created_at": "2026-05-21T10:19:07.323865Z",
"data": {
"invoice_id": "80c4a742-5a02-4fc6-b5cf-ced661e32f84",
"payment_attempt_id": "c9b49fa6-6219-4752-9a29-91808de9104f",
"psp_ref": "f5227509-370d-4f5c-9d76-b99e4be9ea1d"
},
"id": "283558f8-e40a-40d9-b371-65d4216ce6a1",
"type": "invoice.paid"
}

---

{
"id": "5b6e9a2e-a87a-465f-9dce-26a2ee5a956c",
"customer_id": "fb02bcbb-deb0-44aa-9de7-22631203e39f",
"state": "open",
"total_cents": 1000,
"currency": "USD",
"due_date": null,
"created_at": "2026-05-21T10:20:49.929551Z",
"updated_at": "2026-05-21T10:20:49.929551Z",
"line_items": [
{
"id": "c9b3acf4-2b38-4dec-a197-89c822aa800a",
"description": "x",
"quantity": 1,
"unit_amount_cents": 1000,
"position": 0
}
]
}

{
"id": "6360defa-c506-4e03-9026-0b22a245a73c",
"invoice_id": "5b6e9a2e-a87a-465f-9dce-26a2ee5a956c",
"status": "failed",
"psp_ref": null,
"failure_code": "card_declined",
"created_at": "2026-05-21T10:21:47.204249Z",
"completed_at": "2026-05-21T10:21:47.315037Z"
}
