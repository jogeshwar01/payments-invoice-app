# Invoice & Payment Service

A minimal invoice & payment service: businesses issue invoices, customers pay
them, and state changes fan out as signed webhooks. The interesting work is
in the state machine, the payment-concurrency story, and the failure-mode
handling.

**Stack:** Rust + Actix-web + sqlx + Postgres + docker compose.

The primary deliverable is [`DESIGN.md`](DESIGN.md). Read that first.

## Demo Video

_TODO: replace with Loom link before submission._

## Run it

```sh
docker compose up --build
```

Three services come up:

| Service    | Host port | Purpose                                                |
| ---------- | --------: | ------------------------------------------------------ |
| `postgres` |      7000 | database (migrations run on `api` startup)             |
| `mock-psp` |      7001 | fake payment processor, behaviour driven by card_token |
| `api`      |      7002 | the service under test                                 |

`api` waits on `postgres` health (`pg_isready`) and `mock-psp` health
(`/health`). No manual setup steps.

## End-to-end curl flow

Replace `$KEY` and `$INVOICE_ID` as you go.

### 1. Bootstrap a business (returns the plaintext API key — shown once)

```sh
curl -s -X POST http://localhost:7002/v1/businesses \
  -H "Content-Type: application/json" \
  -d '{"name":"Acme Corp"}' | jq
```

```json
{
  "id": "...",
  "name": "Acme Corp",
  "api_key": "sk_live_AbCdEf...",
  "api_key_prefix": "sk_live_AbCdEf12"
}
```

Set `KEY=sk_live_...` in your shell.

### 2. Create a customer

```sh
curl -s -X POST http://localhost:7002/v1/customers \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"Jane Doe","email":"jane@example.com"}' | jq
```

### 3. Create an invoice (auto-finalize so it can be paid immediately)

```sh
curl -s -X POST 'http://localhost:7002/v1/invoices?finalize=true' \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "customer_id": "<customer id>",
    "line_items": [
      {"description":"Pro plan",  "quantity":1, "unit_amount_cents":4900},
      {"description":"Add-on",    "quantity":2, "unit_amount_cents":1200}
    ]
  }' | jq
```

`total_cents` is computed server-side (`4900 + 2 * 1200 = 7300`). Client
totals are not accepted.

### 4. Attempt a successful payment

```sh
curl -s -X POST "http://localhost:7002/v1/invoices/$INVOICE_ID/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: pay-001" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_success"}' | jq
```

Returns `status: succeeded`. Re-running with the same `Idempotency-Key`
returns the cached response without calling the PSP again. Re-running on a
paid invoice with a _new_ key returns `422 invalid_state_for_payment`.

### 5. Attempt a failing payment (declined card)

Create a fresh invoice first (the previous one is now paid).

```sh
curl -s -X POST "http://localhost:7002/v1/invoices/$INVOICE_ID/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: pay-002" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_card_declined"}' | jq
```

Returns `status: failed`, `failure_code: card_declined`. The invoice goes
back to `open`, so a retry with a different `Idempotency-Key` is allowed.

### Other tokens

- `tok_insufficient_funds` — fails with `insufficient_funds`.
- `tok_timeout` — PSP sleeps 30 s. Our endpoint returns `202 pending` after
  ~5 s; the reconciler resolves it within ~20 s and flips to `paid`.
- `tok_network_error` — PSP returns 500. Endpoint returns `202 pending`;
  reconciler tries `GET /psp/charges/{attempt_id}` and the attempt stays
  pending until you intervene (the PSP returns 404 for this case, so the
  attempt sits as pending — see DESIGN.md §3 failure mode (b)).

## Register a webhook endpoint

```sh
curl -s -X POST http://localhost:7002/v1/webhook_endpoints \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"url":"https://webhook.site/<your-id>"}' | jq
```

Returns a `whsec_…` signing secret (shown once). Subsequent state changes
fan out to all active endpoints with header
`Webhook-Signature: t=<unix_ts>,v1=<hex(HMAC-SHA256(secret, "{t}.{body}"))>`.

## API reference

See [`openapi.yaml`](openapi.yaml) for the full spec.

Error format (consistent across all routes):

```json
{
  "error": {
    "type": "invalid_state_transition",
    "message": "...",
    "request_id": "..."
  }
}
```

## Tests

```sh
docker compose up -d postgres mock-psp
cargo test --workspace
```

Three integration tests, all hitting a live Postgres and mock PSP:

- `concurrency.rs` — 20 concurrent `/pay` on the same invoice (distinct
  idempotency keys). Exactly one succeeds; the rest get 409
  `payment_in_progress`. Final invoice state is `paid` and exactly one
  `payment_attempts` row has `status='succeeded'`.
- `idempotency.rs` — 10× repeated `/pay` with the same key returns the same
  response; the mock PSP reports exactly one charge call. Same key with a
  different body returns 409 `idempotency_key_conflict`.
- `psp_failure.rs` — `tok_timeout` returns 202 within 6 s and converges to
  `paid` after the reconciler runs.

State-machine unit tests live in
`crates/api/src/domain/invoice_state.rs::tests`.

## Project layout

```
.
├── crates/
│   ├── api/         # main service (api binary + lib for tests)
│   └── mock-psp/    # mock payment processor
├── migrations/      # 0001..0004 SQL files, run by sqlx::migrate on startup
├── DESIGN.md        # primary deliverable
├── AI_USAGE.md
├── openapi.yaml
└── docker-compose.yml
```

## Notes for graders

- The `POST /v1/businesses` route has no auth on purpose — it's the demo
  bootstrap. In production this would be a console-only operation.
- We use `sqlx`'s runtime query API (no compile-time DB checks) so the build
  doesn't require Postgres running. The trade-off is no compile-time SQL
  validation; we mitigate with integration tests.
- The mock PSP exposes `/psp/_debug/call_count` and `/psp/_debug/reset` for
  test convenience. These would obviously not exist in a real PSP.
