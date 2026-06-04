# Assignment Map

## What they are evaluating

The assignment is not mainly about building many endpoints. It is about payments-adjacent backend judgment:

- clear data model and tenant boundary;
- integer-only money handling;
- invoice state machine with invalid transitions rejected;
- no double charge under concurrent `/pay`;
- idempotency with same-key replay and different-body conflict;
- safe behavior when the PSP is slow, crashes, or returns an ambiguous error;
- signed webhooks that do not block API responses;
- honest design documentation and AI disclosure.

## Requirement-by-requirement mapping

### API key authentication

Implemented in `crates/api/src/auth.rs`.

- Keys look like `dodo_sk_live_<random>`.
- The database stores `SHA-256(plaintext_key)` in `api_keys.key_hash`.
- A short prefix is stored for display.
- Auth is an Actix `FromRequest` extractor: it reads `Authorization: Bearer ...`, hashes the token, looks up an unrevoked key, and produces `BusinessCtx`.

Defense:

- API keys are high-entropy random secrets, so fast SHA-256 is acceptable. Bcrypt/argon2 are for low-entropy human passwords.
- Plaintext is returned once during creation and not stored.
- The blast radius is business-wide in the MVP. Per-key scopes are a production feature.

### Customers

Implemented in `crates/api/src/routes/customers.rs`.

- Create, get, list.
- Every query is scoped by `business_id`.
- Email is not globally unique; `(business_id, email)` is indexed for lookup.

### Invoices

Implemented in `crates/api/src/routes/invoices.rs`.

- Create with line items.
- Server computes `total_cents`.
- No client total is accepted.
- Uses `Cents` newtype and checked arithmetic.
- Get and list, with optional state filter.
- State is constrained both in app code and the DB `CHECK`.

### Payment attempts

Implemented in `crates/api/src/routes/payments.rs`.

- `POST /v1/invoices/{id}/pay` requires `Idempotency-Key`.
- Creates one `payment_attempts` row.
- Calls mock PSP over HTTP using `PspClient`.
- Handles success, failure, timeout, and network ambiguity.
- Uses a two-transaction flow so no database lock is held during network I/O.

### Invoice state machine

Implemented in `crates/api/src/domain/invoice_state.rs`.

States:

- `draft`
- `open`
- `processing`
- `paid`
- `void`
- `uncollectible`

Terminal:

- `paid`
- `void`
- `uncollectible`

Important design point:

- `processing` is load-bearing. It represents "a payment attempt is currently in flight" and prevents a second request with a different idempotency key from also calling the PSP.

### Webhooks

Implemented in `crates/api/src/webhooks`.

- Endpoints are created with a per-endpoint secret.
- Events are written to `outbox_events` in the same transaction as the state change.
- Dispatcher fans out to `webhook_deliveries`.
- Deliveries are signed with HMAC-SHA256.
- Retries use `30s, 2m, 10m, 1h, 6h, 24h`.
- API response path does not wait for webhook delivery.

### PostgreSQL with migrations

Implemented in `migrations/0001_init.sql` through `0004_webhooks.sql`.

Key constraints:

- API key hash unique index.
- Invoice state `CHECK`.
- Money non-negative `CHECK`.
- Payment status `CHECK`.
- Unique `(business_id, idempotency_key)`.
- Partial indexes for pending attempts and pending webhooks.

### Docker compose

Implemented in `docker-compose.yml`.

Services:

- `postgres` on host port 7000.
- `mock-psp` on host port 7001.
- `api` on host port 7002.

### README, OpenAPI, AI disclosure, video

Present:

- `README.md`
- `openapi.yaml`
- `AI_USAGE.md`
- Demo video links in README.

## What to emphasize in the interview

The strongest parts of the assignment:

- The payment correctness story is specific, not hand-wavy.
- Idempotency is backed by a database unique constraint, not only application memory.
- The PSP call is outside database transactions.
- Timeout and crash recovery use the same mechanism: PSP lookup by our `attempt_id`.
- Webhooks use outbox, not best-effort inline HTTP.
- The design explicitly cuts non-essential features.

The weaker parts to own:

- `POST /v1/businesses/{id}/api_keys` is unauthenticated in the MVP and should be admin-only or removed.
- `tok_network_error` can leave a pending attempt forever because the mock PSP returns 500 without storing an outcome.
- Some docs say `OsRng`, while code uses `rand::thread_rng()`; the code still uses a CSPRNG, but the docs should match.
- There is no request-id middleware, only per-error synthesized IDs.
- Webhook endpoint URL validation is only prefix validation; production needs SSRF protection.

## Assumptions made

- Single currency: USD.
- Money is integer cents.
- One successful payment pays the whole invoice.
- No partial payments, refunds, tax, subscription logic, dunning, or FX.
- Idempotency keys are treated as business-scoped.
- Webhooks are at-least-once; receivers must dedupe by event ID.
- API keys have full business scope.
- Zero-dollar line items are allowed by current validation because `unit_amount_cents >= 0`; production should decide whether zero-total invoices should auto-close or be rejected.
