# Payment Correctness And Idempotency

This is the most important interview topic. Be able to explain it without reading.

## Core invariant

For one invoice, at most one successful charge should be created by this service.

Mechanisms:

- `SELECT ... FOR UPDATE` on the invoice row serializes competing payment attempts.
- Invoice state `processing` blocks a second new payment while the first one is in flight.
- `UNIQUE (business_id, idempotency_key)` serializes retries with the same idempotency key.
- `request_hash` rejects same-key/different-body misuse.
- PSP charge uses our `attempt_id` as PSP idempotency key, so reconciliation can look up an old attempt instead of recharging.

## Tx A

Short database transaction before network I/O.

Steps:

1. Lock invoice row by `id` and `business_id`.
2. Read existing `payment_attempts` row by `(business_id, idempotency_key)`.
3. If found with different `request_hash`, return `409 idempotency_key_conflict`.
4. If found and completed, return cached response.
5. If found and pending, return `202 pending`.
6. If no existing attempt, require invoice state to be `open`.
7. Insert `payment_attempts(status='pending')`.
8. Update invoice `open -> processing`.
9. Commit.

Important phrase:

> "Tx A claims the invoice for one payment attempt without doing external I/O."

## PSP call

Runs after Tx A commits.

Key choices:

- Client timeout is 5 seconds.
- No retries in the foreground client.
- Request includes `idempotency_key = attempt_id`.

Why no foreground retries:

> "Retrying a charge after timeout is dangerous because timeout does not mean the PSP did not process it. The safe action is to leave the attempt pending and reconcile by lookup."

## Tx B

Short database transaction after known PSP result.

Steps:

1. Lock invoice row.
2. Lock payment attempt row.
3. If attempt is already final, no-op. This makes foreground and reconciler races safe.
4. On success:
   - mark attempt `succeeded`;
   - store `psp_ref`;
   - move invoice `processing -> paid`;
   - enqueue `invoice.paid`.
5. On failure:
   - mark attempt `failed`;
   - store `failure_code`;
   - move invoice `processing -> open`;
   - enqueue `invoice.payment_failed`.
6. Commit.

## Required failure-mode answers

### A. Two clients call `/pay` for the same invoice at the same time

Different idempotency keys:

- Both try to lock the same invoice row.
- Postgres lets one transaction proceed.
- Winner inserts a pending attempt and moves invoice `open -> processing`.
- Loser wakes after the winner commits.
- Loser sees invoice state `processing`.
- Loser returns `409 payment_in_progress`.
- Loser never calls the PSP.

Same idempotency key:

- The unique index on `(business_id, idempotency_key)` is the final guard.
- One insert wins.
- The loser gets unique violation `23505`, re-reads the existing attempt, and returns the replay/pending response.

Short answer:

> "The invoice row lock handles different-key concurrency; the idempotency unique index handles same-key concurrency."

### B. PSP times out

For `tok_timeout`:

- Mock PSP sleeps 30 seconds then succeeds.
- API client timeout fires at around 5 seconds.
- Handler returns `202 Accepted` with attempt still `pending`.
- Invoice remains `processing`.
- Reconciler later calls `GET /psp/charges/{attempt_id}`.
- Once PSP has stored success, reconciler runs Tx B and moves invoice to `paid`.
- Caller learns by polling invoice or receiving `invoice.paid` webhook.

Important:

> "We do not revert to `open` on timeout, because timeout is ambiguous. Reverting would allow a second card attempt while the first one may still succeed."

### C. PSP succeeds but service crashes before persisting success

State before crash:

- Tx A committed.
- Invoice is `processing`.
- Payment attempt is `pending`.
- PSP may have stored a success under `attempt_id`.
- Tx B did not run.

On restart:

- Reconciler scans stale pending attempts.
- It calls PSP lookup by `attempt_id`.
- PSP returns the original success.
- Reconciler runs Tx B and marks paid.

Why no double charge:

- Reconciliation uses GET lookup, not POST charge.
- The same attempt is not reissued as a new charge.
- A new `/pay` while invoice is `processing` returns conflict before PSP call.

### D. Idempotency key reused with a different body

Implementation:

- Body is canonicalized to JSON with `card_token`.
- SHA-256 hash is stored in `payment_attempts.request_hash`.
- Reuse with different `card_token` produces a different hash.
- Handler returns `409 idempotency_key_conflict`.

Why:

> "Returning the old response for a different request is misleading. Treating it as a new request violates idempotency. Conflict is the safest behavior."

### E. Paid invoice receives another `/pay`

Implementation:

- Tx A locks invoice.
- State machine rejects `Pay` unless state is `open`.
- Paid is terminal.
- Handler returns `422 invalid_state_for_payment`.
- No PSP call is made.

## Double-spend explanation

Potential double-spend scenarios:

- Two clients race with different keys.
- Client retries after timeout.
- Service crashes after PSP success.
- Same key is reused with a different body.
- Webhook receiver fails and causes retry confusion.

Why they are handled:

- Different-key race: invoice row lock and `processing`.
- Retry after timeout: same idempotency key returns pending; new key sees `processing`.
- Crash after PSP success: lookup by `attempt_id`.
- Same-key different body: request hash conflict.
- Webhook retry: webhook delivery does not mutate invoice/payment state.

## Why row lock over alternatives

Advisory lock:

- Works, but less visible and easier to misuse.
- Lock namespace is separate from the row being protected.
- Harder to inspect/debug.

Serializable isolation:

- Correct but forces retry logic around serialization failures.
- Broader tool than needed.
- Row-level lock is direct and local to one invoice.

Optimistic version column:

- Could work.
- Losers would detect stale version and retry.
- For payment, blocking on the row lock is simpler than race-then-retry.

Good answer:

> "I chose the simplest primitive that protects exactly the hot resource: the invoice row."

## Network error nuance

Actual implementation:

- `PspError::NetworkError` returns `202 pending`.
- It does not immediately mark failed.
- This is conservative because a network error may mean "PSP processed it but response was lost."

Mock-specific gap:

- `tok_network_error` returns 500 without storing a charge.
- PSP lookup will return 404 forever.
- That leaves the attempt pending unless an operator or timeout policy resolves it.

How to answer if challenged:

> "The conservative payment answer is correct for ambiguous network errors: do not re-open the invoice immediately if the PSP might still charge. The mock's `tok_network_error` is a known MVP gap because it is actually a definitive 500 that never stores an outcome. In production I would distinguish definitive processor failures from ambiguous transport failures, add an attempt expiry/manual review state, and alert on pending attempts older than an SLA."

## Idempotency key scope

Current scope:

- Unique per business: `(business_id, idempotency_key)`.

Tradeoff:

- This is strict and simple.
- It means a client cannot reuse the same idempotency key for two different invoices in the same business.
- Some APIs scope idempotency keys per endpoint or route.
- The unique-violation fallback should re-check `request_hash` and existing `invoice_id`; see [10_known_gaps.md](10_known_gaps.md).

Defensible answer:

> "I scoped it per business because it is easy to reason about and avoids accidental cross-resource reuse. If the API grew, I might include endpoint path or resource type in the uniqueness key, but then the semantics must be documented very carefully."

## What if Tx B fails after marking attempt but before webhook event

In current code:

- Attempt update, invoice update, and outbox insert all happen in the same Tx B.
- If Tx B rolls back, none of them persist.
- If it commits, all of them persist.

That is the outbox guarantee.
