# Request flow & curl reference

End-to-end map of the system, then every endpoint as a curl call in the order
you'd typically use them.

## How a request flows

```
client ──HTTP──► api (Actix-web)
                  │
                  │ 1. Auth middleware (auth.rs)
                  │    SHA-256(bearer) → SELECT api_keys WHERE key_hash=$1
                  │    AND revoked_at IS NULL → business_id
                  │    Attached to request as BusinessCtx.
                  │
                  │ 2. Handler runs (routes/*.rs)
                  │
                  │    Read: SELECT scoped by business_id
                  │    Write: BEGIN; SELECT FOR UPDATE; ...; INSERT outbox_events; COMMIT
                  │
                  │ 3. Returns response (consistent error JSON on failure)
                  │
                  ▼
              postgres
                  ▲
                  │ background workers (in-process, tokio tasks):
                  │
                  │  • dispatcher.rs: outbox_events → webhook_deliveries
                  │    POST signed payload (HMAC-SHA256). On non-2xx, schedule
                  │    next attempt at 30s/2m/10m/1h/6h/24h.
                  │
                  │  • reconciler.rs: pending payment_attempts older than 10s
                  │    GET /psp/charges/{attempt_id} → if outcome known,
                  │    run Tx B to finalise.
                  │
                  ▼
         mock-psp (POST /psp/charges, GET /psp/charges/{id})
```

## Domain summary

| Resource | Owned by | Notable |
|---|---|---|
| business | — | auth boundary |
| api_key | business | SHA-256 stored, plaintext shown once |
| customer | business | name + email |
| invoice | business + customer | line items, server-computed total, state |
| payment_attempt | business + invoice | UNIQUE(business_id, idempotency_key) |
| webhook_endpoint | business | per-endpoint HMAC secret |
| outbox_event | business | written in same tx as state change |
| webhook_delivery | outbox_event + webhook_endpoint | retry state |

## Invoice state machine (recap)

```
draft ─finalize─► open ─pay─► processing ─psp_success─► paid (terminal)
   │                ▲              │                         
   ▼                └──psp_failure─┘                         
  void                                                       
(terminal)                          
                  open ─mark_uncollectible─► uncollectible (terminal)
                  open ─void─► void (terminal)
```

## Bring the stack up

```sh
docker compose up --build
```

Ports:

| Service | Host port | Container port |
|---|---:|---:|
| postgres | 7000 | 5432 |
| mock-psp | 7001 | 8081 |
| api | 7002 | 8080 |

`api` waits on `postgres` (`pg_isready`) and `mock-psp` (`/health`) before
starting; no manual setup steps.

---

# Curl reference (chronological)

Assumes `jq` is available. Replace `<...>` placeholders with values from the
prior step.

## 0. Health

```sh
curl -fsS http://localhost:7002/health
```

```json
{"status":"ok"}
```

## 1. Bootstrap: create a business

No auth. Returns plaintext API key (shown once).

```sh
curl -fsS -X POST http://localhost:7002/v1/businesses \
  -H "Content-Type: application/json" \
  -d '{"name":"Acme Corp"}' | jq
```

Response:

```json
{
  "id": "b1...",
  "name": "Acme Corp",
  "api_key": "dodo_sk_live_VUZCWqn-...",
  "api_key_prefix": "dodo_sk_live_VUZCWqn-"
}
```

Store the key for the rest of the flow:

```sh
KEY="dodo_sk_live_..."   # from response above
```

## 1b. (Optional) Create an additional API key for a business

```sh
curl -fsS -X POST http://localhost:7002/v1/businesses/<business_id>/api_keys \
  -H "Content-Type: application/json" \
  -d '{"name":"second key"}' | jq
```

## 2. Register a webhook endpoint

Optional — but if you want to see deliveries, do this before creating
anything else. The secret is returned once.

```sh
curl -fsS -X POST http://localhost:7002/v1/webhook_endpoints \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"url":"https://webhook.site/<your-id>"}' | jq
```

Response includes `signing_secret: "whsec_..."`. The receiver should compute:

```
sig = "t=<unix_ts>,v1=" + hex(HMAC-SHA256(secret, "{t}.{body}"))
```

and reject if `|now - t| > 300s`.

## 2b. List webhook endpoints

```sh
curl -fsS http://localhost:7002/v1/webhook_endpoints \
  -H "Authorization: Bearer $KEY" | jq
```

Returns only the secret *prefix*, never the full secret.

## 2c. Delete a webhook endpoint

```sh
curl -fsS -X DELETE http://localhost:7002/v1/webhook_endpoints/<id> \
  -H "Authorization: Bearer $KEY"
```

## 3. Create a customer

```sh
curl -fsS -X POST http://localhost:7002/v1/customers \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"Jane Doe","email":"jane@example.com"}' | jq
```

```sh
CID="<customer id from response>"
```

## 3b. List customers (most recent 100)

```sh
curl -fsS http://localhost:7002/v1/customers \
  -H "Authorization: Bearer $KEY" | jq
```

## 3c. Get one customer

```sh
curl -fsS http://localhost:7002/v1/customers/<id> \
  -H "Authorization: Bearer $KEY" | jq
```

## 4. Create an invoice

`finalize=true` puts it straight in `open` so it can be paid; omit it to
create as `draft` and finalize separately. Server computes `total_cents`.

```sh
curl -fsS -X POST "http://localhost:7002/v1/invoices?finalize=true" \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d "{
    \"customer_id\": \"$CID\",
    \"line_items\": [
      {\"description\":\"Pro plan\",\"quantity\":1,\"unit_amount_cents\":4900},
      {\"description\":\"Add-on\",\"quantity\":2,\"unit_amount_cents\":1200}
    ]
  }" | jq
```

`total_cents` will be `7300` (4900 + 2×1200). Emits `invoice.created`.

```sh
IID="<invoice id from response>"
```

## 4b. Create as draft, then finalize

```sh
INV=$(curl -fsS -X POST "http://localhost:7002/v1/invoices" \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d "{\"customer_id\":\"$CID\",\"line_items\":[{\"description\":\"x\",\"quantity\":1,\"unit_amount_cents\":500}]}")
DRAFT_ID=$(echo "$INV" | jq -r .id)

curl -fsS -X POST "http://localhost:7002/v1/invoices/$DRAFT_ID/finalize" \
  -H "Authorization: Bearer $KEY" | jq .state    # → "open"
```

## 4c. Get an invoice

```sh
curl -fsS http://localhost:7002/v1/invoices/$IID \
  -H "Authorization: Bearer $KEY" | jq
```

## 4d. List invoices (optionally filtered by state)

```sh
curl -fsS "http://localhost:7002/v1/invoices?state=open" \
  -H "Authorization: Bearer $KEY" | jq
```

Valid states: `draft`, `open`, `processing`, `paid`, `void`, `uncollectible`.

## 4e. Void an invoice

```sh
curl -fsS -X POST "http://localhost:7002/v1/invoices/$IID/void" \
  -H "Authorization: Bearer $KEY" | jq .state    # → "void"
```

Only `draft` and `open` can be voided. Terminals reject with 422.

## 5. Pay an invoice — happy path

`Idempotency-Key` is **required**.

```sh
curl -fsS -X POST "http://localhost:7002/v1/invoices/$IID/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: pay-001" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_success"}' | jq
```

Response:

```json
{
  "id": "...",
  "invoice_id": "...",
  "status": "succeeded",
  "psp_ref": "...",
  "failure_code": null,
  "created_at": "...",
  "completed_at": "..."
}
```

Emits `invoice.paid`. Invoice transitions `open → processing → paid`.

## 5b. Pay — replay same idempotency key (cached response)

```sh
curl -fsS -X POST "http://localhost:7002/v1/invoices/$IID/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: pay-001" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_success"}' | jq
```

Identical response. No new PSP call. Same `id`, same `psp_ref`.

## 5c. Pay — same key, different body → 409

```sh
curl -sS -w '\nHTTP %{http_code}\n' -X POST "http://localhost:7002/v1/invoices/$IID/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: pay-001" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_card_declined"}'
```

```
{ "error": { "type": "idempotency_key_conflict", ... } }
HTTP 409
```

## 5d. Pay — paid invoice with a new key → 422

```sh
curl -sS -w '\nHTTP %{http_code}\n' -X POST "http://localhost:7002/v1/invoices/$IID/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: pay-002" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_success"}'
```

```
{ "error": { "type": "invalid_state_for_payment", ... } }
HTTP 422
```

## 5e. Pay — declined card

On a *fresh* open invoice:

```sh
curl -fsS -X POST "http://localhost:7002/v1/invoices/<fresh_open_invoice>/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: decline-1" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_card_declined"}' | jq
```

Returns `status: failed`, `failure_code: card_declined`. Invoice reverts to
`open`. Emits `invoice.payment_failed`. Retry allowed with a new key.

## 5f. Pay — PSP timeout

```sh
time curl -sS -X POST "http://localhost:7002/v1/invoices/<fresh_open_invoice>/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: timeout-1" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_timeout"}' | jq
```

Returns HTTP 202 in ~5 s with `status: pending`. Invoice is in `processing`.
The reconciler converges within ~30 s when the PSP eventually returns
success.

## 5g. Pay — PSP network error

```sh
curl -sS -X POST "http://localhost:7002/v1/invoices/<fresh_open_invoice>/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: nerr-1" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_network_error"}' | jq
```

Returns 202 pending. The mock PSP returned 500, so the reconciler's lookup
gets 404 — the attempt sits as `pending`. In a real system the PSP would
record the (failed) attempt and the reconciler would resolve it; with our
mock, this models the "PSP saw nothing" case and human intervention would
be needed.

## 6. Concurrent pays (concurrency check)

20 parallel pays with distinct keys → exactly one succeeds:

```sh
TMPDIR=$(mktemp -d)
for i in $(seq 1 20); do
  (curl -sS -o /dev/null -w "%{http_code}\n" -X POST \
    "http://localhost:7002/v1/invoices/<fresh_open_invoice>/pay" \
    -H "Authorization: Bearer $KEY" \
    -H "Idempotency-Key: c-$i" \
    -H "Content-Type: application/json" \
    -d '{"card_token":"tok_success"}' > "$TMPDIR/$i") &
done
wait
sort "$TMPDIR"/* | uniq -c
# expect: 1 × 200, 19 × 409
rm -rf "$TMPDIR"
```

## 7. Watch background workers

```sh
docker compose logs -f api | grep -E 'reconciler|webhook|psp'
```

You'll see lines like:
- `psp call starting`
- `psp timeout — leaving attempt pending` (for tok_timeout)
- `reconciler: PSP says succeeded` (after the 30s sleep)
- `webhook delivered`

## 8. PSP debug endpoints (for testing only)

```sh
# How many real charge calls has the PSP processed?
curl -fsS http://localhost:7001/psp/_debug/call_count | jq

# Reset PSP state (clears stored charges and counters).
curl -fsS -X POST http://localhost:7001/psp/_debug/reset

# Look up a charge by our attempt_id (what the reconciler uses).
curl -fsS http://localhost:7001/psp/charges/<attempt_id> | jq
```

These would not exist in a real PSP.

---

## Error format (consistent across all routes)

```json
{
  "error": {
    "type": "invalid_state_transition",
    "message": "...",
    "request_id": "..."
  }
}
```

Common `type` values:

| Type | HTTP | When |
|---|---:|---|
| `bad_request` | 400 | malformed body / missing required field |
| `unauthorized` | 401 | missing or revoked API key |
| `not_found` | 404 | resource not found, or wrong business scope |
| `idempotency_key_conflict` | 409 | same key, different body |
| `payment_in_progress` | 409 | another `/pay` is in flight |
| `invalid_state_transition` | 422 | finalize/void from wrong state |
| `invalid_state_for_payment` | 422 | `/pay` on non-`open` invoice |
| `internal_error` | 500 | bug — check logs |

## Tear down

```sh
docker compose down       # keep volume (data persists)
docker compose down -v    # wipe volume (clean slate)
```
