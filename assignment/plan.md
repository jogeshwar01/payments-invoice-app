# Dodo Payments — Invoice & Payment Service

## Context

The assignment is a Dodo Payments backend take-home: build a minimal Invoice & Payment Service with a mock PSP, signed webhooks, and a strong story around money, state, and concurrency. The graded artifacts are `DESIGN.md`, code correctness, a video walkthrough, and `AI_USAGE.md`. Time budget is 4–6 hours of focused work.

Stack (per user): **Rust + Actix-web + sqlx + Postgres + docker compose**. Mock PSP is a second binary in the same workspace.

The interesting problems are: (a) concurrent `/pay` on the same invoice without double-charging, (b) the PSP being slow/dropped without corrupting state, (c) idempotency that resists key+body mismatch, (d) signed webhooks delivered out-of-band with retries, and (e) honest API key handling.

The plan below is the implementation contract. Each bullet maps to a file or migration to write. The verification section at the end is how I'll know it's done.

---

## Repository Layout

```
dodo/
├── Cargo.toml                  # workspace: [api, mock-psp, shared]
├── Cargo.lock
├── docker-compose.yml
├── Dockerfile                  # multi-stage; one image, two binaries
├── .env.example
├── migrations/                 # sqlx migrate
│   ├── 0001_init.sql
│   ├── 0002_invoices.sql
│   ├── 0003_payment_attempts.sql
│   └── 0004_webhooks.sql
├── crates/
│   ├── api/                    # main Actix service (bin: dodo-api)
│   │   └── src/
│   │       ├── main.rs
│   │       ├── config.rs
│   │       ├── error.rs
│   │       ├── auth.rs
│   │       ├── db.rs
│   │       ├── money.rs        # Cents(i64) newtype
│   │       ├── psp_client.rs
│   │       ├── webhooks/
│   │       │   ├── signer.rs
│   │       │   ├── dispatcher.rs   # background worker
│   │       │   └── reconciler.rs   # pending-attempt reconciler
│   │       ├── domain/
│   │       │   └── invoice_state.rs    # state machine
│   │       └── routes/
│   │           ├── customers.rs
│   │           ├── invoices.rs
│   │           ├── payments.rs
│   │           ├── webhook_endpoints.rs
│   │           └── bootstrap.rs        # POST /v1/businesses (no-auth bootstrap, for demo)
│   └── mock-psp/               # bin: mock-psp
│       └── src/main.rs
├── tests/                      # integration tests against a real Postgres + mock PSP
│   ├── helpers/mod.rs
│   ├── concurrency.rs
│   ├── idempotency.rs
│   └── psp_failure.rs
├── DESIGN.md
├── AI_USAGE.md
├── README.md
└── openapi.yaml
```

A single Dockerfile produces both binaries (cheap, one cargo build). `docker-compose.yml` runs `postgres`, `mock-psp`, `api` (with `sqlx migrate run` on startup), and uses health checks to gate startup order.

---

## Data Model (migrations)

All money is `BIGINT` cents. IDs are `UUID` v4 generated in app code (avoids `gen_random_uuid()` extension dance and keeps inserts deterministic for testing).

**0001_init.sql** — `businesses`, `api_keys`
- `businesses(id UUID PK, name TEXT, created_at TIMESTAMPTZ)`
- `api_keys(id UUID PK, business_id UUID FK, key_hash BYTEA NOT NULL, key_prefix TEXT NOT NULL, name TEXT, created_at, revoked_at TIMESTAMPTZ NULL)`
  - Index on `key_hash` (lookup at auth time).
  - `key_prefix` stored plaintext for dashboard display (`dodo_sk_live_abc1…`).

**0002_invoices.sql** — `customers`, `invoices`, `invoice_line_items`
- `customers(id, business_id, name, email, created_at)` — `(business_id, email)` indexed.
- `invoices(id, business_id, customer_id, state TEXT, total_cents BIGINT, currency TEXT DEFAULT 'USD', due_date DATE NULL, created_at, updated_at, version INT DEFAULT 0)`
  - `state` is a CHECK-constrained string: `draft|open|processing|paid|void|uncollectible`.
  - Index on `(business_id, state, created_at DESC)` for list-by-state filter.
  - `version` reserved for optimistic concurrency on non-payment edits.
- `invoice_line_items(id, invoice_id FK ON DELETE CASCADE, description, quantity INT, unit_amount_cents BIGINT, position INT)`.

**0003_payment_attempts.sql**
- `payment_attempts(id, invoice_id, business_id, idempotency_key TEXT NOT NULL, request_hash BYTEA NOT NULL, status TEXT, psp_ref TEXT NULL, failure_code TEXT NULL, response_json JSONB NULL, created_at, completed_at TIMESTAMPTZ NULL)`
  - `UNIQUE (business_id, idempotency_key)` — this is the idempotency primitive.
  - Index on `(status, created_at)` for the reconciler to scan stale `pending` rows.
- `status ∈ {pending, succeeded, failed}`.

**0004_webhooks.sql**
- `webhook_endpoints(id, business_id, url, signing_secret BYTEA, active BOOL, created_at)`.
- `outbox_events(id, business_id, event_type, payload JSONB, created_at, dispatched_at TIMESTAMPTZ NULL)` — written in the same transaction as the state change.
- `webhook_deliveries(id, outbox_event_id, webhook_endpoint_id, attempt_count INT, next_attempt_at TIMESTAMPTZ, last_status_code INT NULL, last_error TEXT NULL, status TEXT, delivered_at TIMESTAMPTZ NULL)`
  - `status ∈ {pending, delivered, failed}`. Partial index on `status='pending' AND next_attempt_at <= now()` for the dispatcher.

---

## Invoice State Machine

States: `draft → open → processing → paid` (happy path), plus terminals `void` and `uncollectible`.

```
              ┌────────┐
              │ draft  │──void──► void (terminal)
              └───┬────┘
            finalize
                  ▼
              ┌────────┐
   ┌─void────│  open  │◄──── psp failure ─────┐
   ▼          └───┬────┘                       │
  void          pay (POST /pay)                │
                  ▼                             │
              ┌──────────┐                      │
              │processing│──── psp success ─► paid (terminal)
              └────┬─────┘
            mark_uncollectible
                  ▼
            uncollectible (terminal)
```

- `processing` is the in-flight payment state. Only `open` invoices can be paid; the `open→processing` transition is gated by `SELECT … FOR UPDATE` on the invoice row.
- PSP failure (declined / network error) flips `processing → open` — the customer can retry.
- PSP timeout leaves the attempt `pending`; the reconciler eventually flips invoice to `paid` or `open`.
- `paid` / `void` / `uncollectible` are terminal. Invalid transitions are rejected with `422 invalid_state_transition` from a single `try_transition()` function in `domain/invoice_state.rs`.

For the demo flow, `POST /v1/invoices` accepts `?finalize=true` to bypass the `draft` step in curl examples. Default behaviour creates as `draft`.

---

## Concurrency & Payment Correctness (the hard section)

`POST /v1/invoices/{id}/pay` flow:

1. Validate `Idempotency-Key` header is present; compute `request_hash = SHA-256(canonicalized body)`.
2. **Transaction A** (short, no I/O inside):
   - `SELECT … FROM invoices WHERE id = $1 AND business_id = $2 FOR UPDATE` — row lock.
   - Check existing `payment_attempts` row by `(business_id, idempotency_key)`:
     - If found + `request_hash` differs → return `409 idempotency_key_conflict`.
     - If found + same hash + `status != pending` → return cached response.
     - If found + same hash + `pending` → return `202 { status: "pending" }` (caller is replaying mid-flight).
   - If invoice state ∉ `{open}` → reject `422 invalid_state_for_payment`.
   - Insert new `payment_attempts` row (`status = pending`). The `UNIQUE (business_id, idempotency_key)` index serializes concurrent inserts with the same key.
   - Transition invoice `open → processing`.
   - COMMIT (release lock).
3. Call PSP over HTTP with a 5-second client timeout.
4. **Transaction B**:
   - `SELECT … FOR UPDATE` on invoice.
   - Update `payment_attempts` row with PSP result.
   - On success: `processing → paid`, write `invoice.paid` event to `outbox_events`.
   - On declined / known failure: `processing → open`, write `invoice.payment_failed` event.
   - COMMIT.
5. Return PSP-mapped response to caller.

**If the PSP times out** (step 3 raises a timeout): leave `payment_attempts.status = pending`, leave invoice in `processing`, return `202 { status: "pending", attempt_id }`. The background **reconciler** scans `pending` attempts older than ~10s, calls `GET /psp/charges/{psp_ref_or_idempotency}` on the mock PSP, and finishes Transaction B accordingly. (To make this work, when we kick off the PSP call we pass our `attempt_id` as the PSP's idempotency key, so on reconcile we can look the charge up even if we never saw the original response.)

**Mapping each failure mode the spec asks about:**

- **(a) Two concurrent `/pay` with different idempotency keys** — both contend on `SELECT FOR UPDATE`. First wins, moves `open → processing`. Second wakes up, sees state ≠ `open`, returns `409 payment_in_progress`. No double charge.
- **(a′) Two concurrent `/pay` with the *same* idempotency key** — `UNIQUE (business_id, idempotency_key)` causes one insert to fail with `23505`. That request retries the read path and returns the in-flight `pending` response.
- **(b) tok_timeout (30 s)** — endpoint returns `202` after ~5 s with `status: pending`. Invoice stays in `processing`. Reconciler converges within ~10–20 s of the PSP eventually succeeding. Caller polls `GET /v1/invoices/{id}` or receives the webhook.
- **(c) PSP returned success but we crashed before Transaction B** — on restart, the reconciler sees the `pending` attempt, calls the PSP lookup endpoint with our `attempt_id` (idempotency key on PSP side), discovers the charge succeeded, and writes `paid`. Customer is charged exactly once because we never re-issue the charge — we always *look up* the existing one by `attempt_id`.
- **(d) Idempotency key reused with different body** — `request_hash` mismatch → `409 idempotency_key_conflict`. We never silently treat a different request as the original.
- **(e) Already-paid invoice receives another `/pay`** — invoice state check in Tx A rejects with `422 invalid_state_for_payment`. No PSP call.

Named mechanism: **row-level lock (`SELECT … FOR UPDATE`) on `invoices` + unique constraint on `(business_id, idempotency_key)` + state-conditional transitions**. Defended against advisory locks (less debuggable, no FK locality) and serializable isolation (retry-on-conflict noise, harder to reason about under load).

---

## Webhook Design

- **Signing**: `Dodo-Signature: t=<unix_ts>,v1=<hex(HMAC-SHA256(secret, "{t}.{body}"))>`. The signed string is `timestamp.body`; receivers compare HMACs in constant time and reject `|now - t| > 5 min` for replay protection.
- **Decoupling**: state-change transactions write to `outbox_events`. A background worker (tokio task in the same process for MVP) reads `outbox_events WHERE dispatched_at IS NULL`, expands to per-endpoint `webhook_deliveries` rows, and POSTs.
- **Retry**: exponential backoff at `30s, 2m, 10m, 1h, 6h, 24h` (6 attempts, ~31h total). Stored in `webhook_deliveries.next_attempt_at`; a single worker scans by partial index.
- **Exhaustion**: `status = failed`. Businesses reconcile via `GET /v1/events?since=…` (returned as part of the cut list if I run out of time — I'll log the spec but stub the endpoint).
- **Events**: `invoice.created`, `invoice.paid`, `invoice.payment_failed`. Minimum required set per the spec.

---

## API Key Model

- **Generation**: 32 random bytes from `OsRng`, base64url-encoded, prefixed `dodo_sk_live_`.
- **Storage**: SHA-256 of the full key in `api_keys.key_hash`. Plaintext is shown ONCE at creation; never persisted.
- **Display prefix**: first 8 chars of the suffix stored in `api_keys.key_prefix` for dashboard listing.
- **Transmission**: `Authorization: Bearer dodo_sk_live_…`. HTTPS in prod (TLS termination at the LB; out of scope for the assignment).
- **Rotation**: multiple active keys per business; create new before revoking old.
- **Revocation**: `revoked_at` timestamp; auth middleware rejects if set.
- **Blast radius**: full business scope. Mitigations discussed in DESIGN.md (no per-key scoping in this MVP — graded restraint).

---

## Endpoints (OpenAPI)

All under `/v1`, all require `Authorization: Bearer …` except `POST /v1/businesses` (bootstrap, no-auth, for demo only; documented as "remove in prod").

- `POST /v1/businesses` → `{id, api_key}` (one-shot, returns plaintext key).
- `POST /v1/customers`, `GET /v1/customers/{id}`, `GET /v1/customers`.
- `POST /v1/invoices` (with `?finalize=true` shortcut), `GET /v1/invoices/{id}`, `GET /v1/invoices?state=open`.
- `POST /v1/invoices/{id}/finalize`, `POST /v1/invoices/{id}/void`.
- `POST /v1/invoices/{id}/pay` — requires `Idempotency-Key`.
- `POST /v1/webhook_endpoints`, `GET /v1/webhook_endpoints`, `DELETE /v1/webhook_endpoints/{id}`.

Mock PSP (separate binary, port 8081):
- `POST /psp/charges` — body `{idempotency_key, amount_cents, card_token}`; behaviour by token per spec.
- `GET /psp/charges/{idempotency_key}` — for reconciliation lookup.

**Error format** (consistent across all routes):
```json
{ "error": { "type": "invalid_state_transition", "message": "...", "request_id": "..." } }
```

---

## Tests (the three the spec mandates)

Run against a real Postgres (docker-compose-provisioned) and the real mock PSP. No mocks of our own DB.

- `tests/concurrency.rs`: bootstraps a business + invoice, fires 20 concurrent `POST /pay` with **distinct** idempotency keys via `tokio::join_all`, asserts exactly 1 succeeds, 19 receive 409, invoice ends in `paid` with exactly 1 `succeeded` attempt.
- `tests/idempotency.rs`: same key, same body, fired 10× sequentially and concurrently — asserts exactly 1 PSP call (mock PSP exposes a `GET /psp/_debug/call_count` for the test), all responses identical. Then same key + different body → 409.
- `tests/psp_failure.rs`: uses `tok_timeout` → asserts endpoint returns 202 within ~6 s (not 30 s), invoice in `processing`, reconciler converges to `paid` within polling window. Uses `tok_network_error` → asserts invoice flips back to `open` and another attempt with a new key succeeds.

---

## Critical files to write

| File | Why it's critical |
|------|-------------------|
| `migrations/0001…0004_*.sql` | Schema is the spine; CHECK constraints + UNIQUE indexes are the safety net for the state machine and idempotency. |
| `crates/api/src/routes/payments.rs` | Houses the two-transaction flow described above. The single hottest correctness path. |
| `crates/api/src/domain/invoice_state.rs` | One pure `try_transition(from, event) -> Result<State, Error>` function. All state changes route through it. |
| `crates/api/src/psp_client.rs` | reqwest client with 5s timeout, retries off (idempotency happens at our layer), parses PSP responses including timeout. |
| `crates/api/src/webhooks/dispatcher.rs` | Outbox → delivery expansion, backoff math, HMAC signing. |
| `crates/api/src/webhooks/reconciler.rs` | Scans `payment_attempts WHERE status='pending' AND created_at < now() - 10s`; resolves via PSP lookup. |
| `crates/api/src/auth.rs` | Middleware: extract bearer, SHA-256, lookup, attach `BusinessCtx` to request extensions. |
| `crates/mock-psp/src/main.rs` | Behaviour map by card token, with `tok_timeout` actually sleeping 30s in a tokio task and `tok_network_error` calling `std::process::abort()` per-connection or returning a 500. |
| `docker-compose.yml` | postgres (with healthcheck), mock-psp, api (depends_on: healthy). |
| `DESIGN.md` | The graded artifact — sections 1–7 mirror the spec exactly. |
| `AI_USAGE.md` | Specific list of AI uses + 3 independent decisions + 1 correction. |
| `README.md` | Run instructions, 4 curl examples (create customer, create invoice, pay-success, pay-decline), Demo Video link placeholder. |
| `openapi.yaml` | Schemas mirroring the request/response shapes used in the routes. |

---

## What I'm explicitly cutting (will be listed in DESIGN.md §6)

- **Per-key scoping / least-privilege API keys** — full business scope only.
- **Background event-history endpoint for missed-webhook reconciliation** — stub the spec, no implementation.
- **Multi-tenant rate limiting** — discussion only.
- **Observability (structured tracing, metrics)** — basic `tracing` logs only; no OpenTelemetry.
- **Refunds / partial payments** — discussion only (spec explicit).

These are the §6 entries; §7 (production gap) will name the top three: observability, rate limiting, audit log.

---

## Verification

End-to-end, before declaring done:

1. `docker compose down -v && docker compose up --build` on a clean machine → all three services healthy, migrations applied.
2. Run the 4 README curl examples in order → bootstrap business, create customer, create invoice, pay with `tok_success` (200), pay with `tok_card_declined` (200 with failed attempt). Inspect `outbox_events` and `webhook_deliveries` tables to confirm events were enqueued and dispatched.
3. Register a webhook endpoint pointing at a `webhook.site` URL → trigger an `invoice.paid` → confirm the signed POST arrives and signature verifies with the stored secret.
4. `cargo test --test concurrency --test idempotency --test psp_failure` → all three pass against the live stack.
5. Manually trigger `tok_timeout`: hit `/pay`, confirm 202 within 6 s, watch reconciler logs flip invoice to `paid`.
6. Hit a paid invoice with `/pay` again → confirm 422.
7. Reuse an idempotency key with a different body → confirm 409.
8. Final pass: re-read DESIGN.md against the spec's seven sections; ensure each failure mode (a)–(e) is answered specifically with mechanisms named.
9. Record the 5–10 min Loom covering architecture, live demo, state machine, and the `tok_timeout` failure-mode walkthrough.
