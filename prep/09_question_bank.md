# Question Bank

## Architecture

### Walk me through the system.

There are three services: API, Postgres, and mock PSP. The API is Rust/Actix and owns auth, customer/invoice/payment routes, and background workers. Postgres is the source of truth for invoice state, payment attempts, idempotency, and webhook delivery state. The mock PSP is a separate HTTP service so timeouts and network failures are real from the API's perspective.

### Why did you split API and mock PSP?

Because the assignment asks us to treat the PSP as an external dependency. A separate service forces us to handle HTTP timeouts, status codes, docker-compose health, and reconciliation instead of pretending a function call can fail like a processor.

### Why background workers in the same process?

It keeps the MVP deployment simple. All worker state is in Postgres, so moving them to separate worker binaries later is straightforward.

## Payments

### How do you prevent double charging?

By combining invoice row locking, an explicit `processing` state, idempotency unique constraints, and PSP lookup by our attempt ID. A second concurrent payment either waits and sees `processing`, or replays the existing idempotent attempt. It does not call the PSP.

### What happens if two clients pay at once?

They contend on `SELECT ... FOR UPDATE` for the invoice. The first moves `open -> processing` and calls the PSP. The second wakes up, sees `processing`, returns `409 payment_in_progress`, and does not call the PSP.

### What if the same idempotency key is sent twice?

If the body is the same, return the existing attempt response. If completed, `200`; if still pending, `202`. If the body differs, return `409 idempotency_key_conflict`.

### What is a subtle idempotency edge case in this code?

The `23505` fallback should verify the existing attempt's `request_hash` and `invoice_id`. It is fine for the normal same-invoice race, but cross-invoice same-key concurrent requests could otherwise replay the wrong existing attempt. Production fix: load and compare both fields before replaying.

### Why store `request_hash`?

Because an idempotency key is only valid for the same request. Without a request hash, same-key/different-body reuse could either incorrectly replay the old response or incorrectly become a new payment.

### What if the PSP times out?

Return `202 pending`, keep invoice `processing`, keep attempt `pending`, and let the reconciler look up the eventual PSP outcome by attempt ID.

### Why not mark timeout as failed?

Timeout is ambiguous. The PSP may have processed the charge but the response did not arrive. Marking failed and reopening the invoice can create a duplicate charge.

### What if PSP succeeds but the API crashes before saving?

The attempt remains pending and invoice remains processing. On restart, the reconciler looks up `attempt_id` at PSP, gets success, and runs Tx B. We do not POST a second charge.

### Why use `attempt_id` as PSP idempotency key?

It creates a stable join key for recovery. If the response is lost, we can ask the PSP what happened to our attempt instead of recharging.

### Why no retries in `PspClient::charge`?

Foreground retries after ambiguous timeout can double charge. The safe retry is a lookup/reconciliation flow.

## State machine

### Why have `processing`?

It is the in-flight payment guard. Without it, after releasing the DB lock before PSP I/O, another request could see the invoice as payable and call the PSP too.

### Why are `paid`, `void`, and `uncollectible` terminal?

They represent business-final states. Corrections should be compensating records like refunds or new invoices, not mutation of historical state.

### Why not create invoices directly as open?

The demo supports `?finalize=true`, but keeping `draft` models a real invoice lifecycle and cleanly separates "building invoice" from "payable invoice."

## Postgres and locking

### What does `SELECT ... FOR UPDATE` do?

It locks selected rows until transaction end. Another transaction trying to lock the same row waits, then sees the latest committed state.

### Why is `READ COMMITTED` okay?

Because the code explicitly locks the row before checking and mutating state. The unique idempotency index handles concurrent same-key inserts.

### Why not serializable isolation?

Serializable is correct but would require retry loops for serialization errors. The conflict is on one invoice row, so a row lock is simpler and more explicit.

### Could this deadlock?

Low risk now because payment code locks invoice first and then attempt. Future code must preserve lock ordering. Production should monitor lock waits and deadlocks.

### What if the DB crashes after Tx A and before PSP call?

If Tx A committed but PSP call never happened, the attempt remains pending and invoice processing. The reconciler lookup will find no PSP charge. Current MVP would leave it pending; production needs a stale pending policy/manual resolution.

## Webhooks

### Why use outbox?

It makes event persistence atomic with state change and decouples API latency from receiver latency.

### What is signed?

The exact raw JSON body prefixed by timestamp: `"{t}.{body}"`.

### How does replay protection work?

The timestamp is included in the signature. Receivers reject signatures outside a 5-minute window.

### What if webhook delivery fails forever?

The delivery is marked failed after the retry schedule. Production should expose event replay/reconciliation and dashboard alerts.

### Could duplicate webhooks happen?

Yes, webhooks are generally at-least-once. Receivers should dedupe by event ID. The service tries to avoid accidental concurrent delivery, but clients must still treat webhooks as at-least-once.

## Auth and security

### Why hash API keys?

A DB dump should not immediately contain usable credentials. Hash lookup still supports authentication.

### Why SHA-256 instead of bcrypt?

API keys are random 32-byte secrets, not human passwords. Slow password hashing adds hot-path latency without meaningful brute-force benefit.

### What is the blast radius of a leaked key?

Full business scope in the MVP. Production needs scopes, rate limits, audit logs, and possibly IP allowlists.

### What is wrong with the current key creation route?

`POST /v1/businesses/{id}/api_keys` is unauthenticated. It is a demo/MVP shortcut and should be admin-only or removed before production.

## Rust

### Why Actix extractors?

They keep cross-cutting validation out of handler bodies. If a route needs auth or idempotency, its function signature says so.

### Why `Cents` newtype?

To avoid mixing arbitrary `i64` values with money and to centralize checked addition/multiplication.

### Is `std::sync::Mutex` okay in async code?

It is okay if the critical section is short and the guard is not held across `.await`. The mock PSP uses it only for quick HashMap access.

### What does `Arc` do here?

It lets the PSP client and config be shared across handlers/tasks by thread-safe reference counting.

## Product/design

### What did you deliberately cut?

Per-key scopes, refunds, partial payments, subscriptions, event replay API, production rate limiting, full observability, audit log, and real PSP integration.

### Top production gaps?

Observability, rate limiting, audit log, stronger security around key management/webhook SSRF, and explicit manual resolution for stuck pending payments.

### What would you change first with more time?

I would fix authenticated key management, add request-id middleware/metrics, and add a manual/admin resolution path for ambiguous pending payment attempts.
