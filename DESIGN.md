# DESIGN.md - Invoice & Payment Service

This document is the primary deliverable. It explains the design judgment
behind the code. It is organised to mirror the seven sections asked for in the
brief.

## 1. Data Model

Eight tables, all FK'd back to `businesses`. Money is `BIGINT` cents
everywhere. IDs are app-generated UUIDv4 - keeps inserts deterministic for
tests and avoids depending on the `pgcrypto`/`uuid-ossp` extensions.

```
businesses ─┬─< api_keys
            ├─< customers ─< invoices ─┬─< invoice_line_items
            │                          └─< payment_attempts
            ├─< webhook_endpoints
            └─< outbox_events ─< webhook_deliveries >─ webhook_endpoints
```

| Table                | Shape (selected)                                                                                                 | Indexes                                                                                                                        | Why                                                                                                                                                           |
| -------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `businesses`         | id, name, created_at                                                                                             | PK                                                                                                                             | Auth boundary.                                                                                                                                                |
| `api_keys`           | id, business_id, **key_hash**, key_prefix, revoked_at                                                            | UNIQUE(key_hash); IDX(business_id)                                                                                             | Storing the SHA-256 hash means a DB dump doesn't leak working credentials. The unique index makes auth a single index probe.                                  |
| `customers`          | id, business_id, name, email                                                                                     | IDX(business_id, email)                                                                                                        | Email isn't unique - businesses may have multiple records for the same person.                                                                                |
| `invoices`           | id, business_id, customer_id, **state**, total_cents, currency, due_date                                         | CHECK(state in ...), IDX(business_id, state, created_at DESC)                                                                  | The CHECK constraint is a belt-and-suspenders defense against bad state strings even if app code regresses. The composite index serves the `?state=` filter.  |
| `invoice_line_items` | invoice_id, description, quantity, unit_amount_cents, position                                                   | IDX(invoice_id, position)                                                                                                      | `total_cents` is computed server-side from these rows; client total is ignored. Cascades on invoice delete (we never expose delete, but the FK is defensive). |
| `payment_attempts`   | id, invoice_id, business_id, **idempotency_key**, **request_hash**, status, psp_ref, failure_code, response_json | **UNIQUE(business_id, idempotency_key)**, IDX(invoice_id, created_at DESC), partial IDX on (created_at) WHERE status='pending' | The unique index is the idempotency primitive; the partial index drives the reconciler.                                                                       |
| `webhook_endpoints`  | id, business_id, url, **signing_secret (BYTEA)**, active                                                         | IDX(business_id) WHERE active                                                                                                  | Secret is opaque bytes; never returned after creation.                                                                                                        |
| `outbox_events`      | id, business_id, event_type, payload (JSONB), dispatched_at                                                      | Partial IDX on (created_at) WHERE dispatched_at IS NULL                                                                        | Outbox pattern: events are written in the same transaction as the state change that emitted them.                                                             |
| `webhook_deliveries` | id, outbox_event_id, webhook_endpoint_id, attempt_count, next_attempt_at, status                                 | Partial IDX on (next_attempt_at) WHERE status='pending'                                                                        | Each event fans out to one delivery row per endpoint. Backoff state lives here.                                                                               |

**Why this shape over alternatives.** The split between `outbox_events` and
`webhook_deliveries` is deliberate. We could store everything on
`webhook_deliveries` and skip the outbox, but then the API would have to know
about every active endpoint when writing, which couples write latency to
endpoint count and breaks if a new endpoint is registered between
state-change and dispatch. Two tables give us: write once cheaply (outbox),
fan out asynchronously (deliveries).

The `payment_attempts.request_hash` field is what catches
"same-idempotency-key-different-body" - the spec's failure-mode (d). Without
it we'd have to choose between silently returning a stale response (wrong)
and rejecting all replays (breaks legitimate retries).

**What would change at 100× scale.**

- Partition `outbox_events` and `webhook_deliveries` by month/week; rotate old
  partitions to cheap storage.
- Move the reconciler and dispatcher into their own processes with their own
  DB credentials (read-replica + targeted writes).
- Add a covering index on `payment_attempts` to serve idempotency lookups
  from index pages alone.
- Stop sharing one Postgres for everything - move webhook delivery state to
  its own logical database so retry storms don't contend with the API path.

## 2. Invoice State Machine

```
                 ┌─────────┐
                 │  draft  │ ──── void ────────────┐
                 └────┬────┘                       │
                  finalize                          ▼
                      ▼                          ┌──────┐
                 ┌─────────┐ ── void ─────────► │ void │ (terminal)
                 │   open  │                    └──────┘
       ┌── PSP   └────┬────┘
       │   failure   pay
       │              ▼
       │       ┌────────────┐
       └────── │ processing │ ── PSP success ──► ┌──────┐
               └────────────┘                    │ paid │ (terminal)
                   ▲                             └──────┘
                   │ mark_uncollectible (open)
                   ▼
              ┌───────────────┐
              │ uncollectible │ (terminal)
              └───────────────┘
```

Transitions and triggers:

| From       | Event             | Trigger                                     | To            |
| ---------- | ----------------- | ------------------------------------------- | ------------- |
| draft      | Finalize          | `POST /v1/invoices/{id}/finalize`           | open          |
| draft      | Void              | `POST /v1/invoices/{id}/void`               | void          |
| open       | Pay               | `POST /v1/invoices/{id}/pay` (Tx A success) | processing    |
| open       | Void              | `POST /v1/invoices/{id}/void`               | void          |
| open       | MarkUncollectible | (admin op; future)                          | uncollectible |
| processing | PspSuccess        | Tx B after PSP returns succeeded            | paid          |
| processing | PspFailure        | Tx B after PSP returns failed/network err   | open          |

**Terminal states:** paid, void, uncollectible. None are reversible at the
state-machine layer. Operational corrections (incorrect void, refund of a paid
invoice) would issue separate compensating records (e.g. a Refund row), not
mutate the historical state.

**Reversibility:** only `processing - open` (PSP failure path) is a real
"reversal," and it represents PSP-side rejection rather than user action. The
canonical forward path is monotonic.

**Invalid transitions** are rejected with `422 invalid_state_transition` from
a single pure function: `try_transition(InvoiceState, TransitionEvent) ->
Result<InvoiceState, InvalidTransition>` in
`crates/api/src/domain/invoice_state.rs`. Every state change in the codebase
routes through it. Unit tests cover the happy path, the
"pay-only-from-open" rule, the no-outgoing-from-terminal rule, and the
PSP-failure revert.

## 3. Payment Correctness & Failure Modes

The hard section. Read this first if you only have time for one.

### Mechanism

**Row-level lock (`SELECT … FOR UPDATE`) on `invoices`** plus **`UNIQUE
(business_id, idempotency_key)` on `payment_attempts`** plus
**state-conditional transitions** in app code.

The lock is the serializer for concurrent attempts on the same invoice; the
unique index is the serializer for retries that share an idempotency key; the
state machine prevents already-final invoices from being re-paid.

Lock is held only inside short transactions. The PSP HTTP call happens
_between_ two transactions, with no DB lock held during network I/O.

### Why these primitives over alternatives

- **Advisory locks** were considered. Rejected because they share a 64-bit
  namespace across the database, lack the locality of an indexed row lock
  (PgAdmin can't show you what's holding them), and require their own release
  discipline.
- **Serializable isolation** was considered. Rejected because the
  conflict-and-retry pattern adds noise to every endpoint, not just the
  payment one, and is hard to reason about under load. Row locks scoped to
  the single hot row give us the same correctness with less radius.
- **Optimistic concurrency on a version column** would work too, and we keep
  a `version` column on invoices for future non-payment edits. For
  payment-flow updates the row lock is simpler: the second writer
  _blocks_ rather than racing-then-retrying.

### Flow

```
   Tx A (short, no network):
     SELECT … FOR UPDATE invoices                              -- serialize
     check existing payment_attempts (business_id, idem_key)
        -> request_hash mismatch  -> 409 idempotency_key_conflict
        -> match, completed       -> replay cached response
        -> match, pending         -> 202 pending
     check state == open                                       -- (or revert)
     INSERT payment_attempts(pending)                          -- UNIQUE: 23505 -> replay
     UPDATE invoices state open -> processing
     COMMIT

   PSP call (5s client timeout, no retries):
     idempotency_key = our attempt_id            -- recoverable via lookup
     succeeded / failed     -> Tx B with that outcome
     timeout                -> return 202; reconciler converges
     network error          -> return 202; reconciler disambiguates via lookup

   Tx B (short):
     SELECT … FOR UPDATE invoices                              -- serialize
     SELECT … FOR UPDATE payment_attempts                      -- guard against reconciler
     if attempt already final, no-op  (replay-safe)
     UPDATE payment_attempts(succeeded|failed)
     UPDATE invoices processing -> paid | open
     INSERT outbox_events
     COMMIT
```

### Failure-mode answers

**(a) Two clients call POST /pay for the same invoice simultaneously.**

Two sub-cases, both safe:

- _Different idempotency keys._ They contend on `SELECT … FOR UPDATE
invoices`. Postgres serialises them. The winner moves the invoice
  `open - processing` and releases the lock. The loser proceeds, sees
  `state == processing`, and returns
  `409 payment_in_progress`. No PSP call is issued on the losing path,
  no double charge.

- _Same idempotency key._ Both pass the lock acquisition (in serial), but
  both attempt to `INSERT payment_attempts` with the same
  `(business_id, idempotency_key)`. The unique index fails the second
  insert with Postgres SQLSTATE `23505`. The handler catches it, re-reads
  the existing attempt, and returns `202 pending` (caller is in the same
  state as the first request from its own POV).

**(b) The mock PSP times out (`tok_timeout`, 30 s).**

The reqwest client has a 5 s total timeout (configurable). When it fires,
`PspError::Timeout` is returned. The handler **does not roll back the
invoice state**. The endpoint returns `202 { status: "pending",
attempt_id, ... }`.

The background reconciler scans `payment_attempts WHERE status='pending'
AND created_at < now() - 10s` every 2 s. It calls
`GET /psp/charges/{attempt_id}` on the PSP (the PSP keys its records by
our idempotency key, so it can serve the eventual outcome). When the PSP
returns success ~30 s after the original request, the reconciler runs Tx
B with that result, transitions the invoice to `paid`, and enqueues the
`invoice.paid` event.

The caller can find out the outcome by:

1. Polling `GET /v1/invoices/{id}` (state will flip from `processing` to
   `paid`).
2. Receiving the `invoice.paid` webhook.

Critically, **the invoice is not stuck**: even if our reconciler is also
down, the next attempt to query state will see `processing` and the next
reconciler tick will resolve it.

**(c) The PSP returns success but our service crashes before persisting.**

The customer is **charged exactly once** - we never re-issue the charge.

When we POST to the PSP we pass our `attempt_id` as its idempotency key.
After the crash, the row is still `(pending, no psp_ref)`. The reconciler
finds it, calls `GET /psp/charges/{attempt_id}`, and the PSP returns the
outcome it stored on the original call. The reconciler runs Tx B with
that outcome - transitions to `paid` and emits the event.

Because we never invoke `POST /psp/charges` again for this attempt
(reconciliation uses GET, and any later re-invocation would carry a
_new_ `attempt_id` from a new `/pay` call, which would block on the row
lock and then bounce on state), there is no path to a duplicate charge.

**(d) Idempotency key reused with a different request body.**

Tx A reads the existing `payment_attempts` row, compares its
`request_hash` (SHA-256 of the canonicalised body) to the incoming hash,
and returns **`409 idempotency_key_conflict`** when they differ.

We deliberately do _not_ silently treat the new body as the original.
Stripe does the same thing - returning a misleading "succeeded" response
for a different request is worse than returning an error.

**(e) An already-paid invoice receives another POST /pay.**

Tx A's state check rejects with `422 invalid_state_for_payment`. No PSP
call is made.

The same rule applies to void and uncollectible invoices - the state
machine's `try_transition(state, Pay)` returns `Err` for anything other
than `Open`.

## 4. Webhook Design

**Signing.** `Dodo-Signature: t=<unix_ts>,v1=<hex(HMAC-SHA256(secret,
"{t}.{body}"))>`. The signed string is `timestamp.body`; receivers
recompute and compare in constant time, then reject signatures where
`|now - t| > 300 s` (replay window). The timestamp is part of the signed
material, so an attacker cannot resign a captured payload with a new
timestamp.

We chose HMAC-SHA256 because every webhook receiver SDK supports it,
verification is cheap, and it has zero key-rotation drama: rotating the
secret is a single UPDATE. Asymmetric signing (Ed25519) was overkill for
this assignment - every receiver would need a public key and a verifier;
we'd own a key-distribution problem.

**Retry policy.** Backoff schedule: `30s, 2m, 10m, 1h, 6h, 24h` - six
attempts spanning ~31 hours. Stored in
`webhook_deliveries.next_attempt_at`. The numbers are tuned so the first
retries happen quickly (most receiver failures are transient), then back
off enough that a multi-hour outage doesn't get hammered.

A delivery is **considered failed** when the receiver returns non-2xx,
times out, or refuses connection. We bump `next_attempt_at` and increment
`attempt_count`. When `attempt_count` reaches the schedule length, we
flip `status` to `failed` and stop.

**Reconciliation.** Businesses recover missed events two ways:

1. The state itself is queryable - they can `GET /v1/invoices/{id}` and
   compare to what they remember.
2. (Not built; in the cut list) A `GET /v1/events?since=…` endpoint
   would let them replay events whose webhooks failed. Spec'd but not
   implemented.

**Why delivery is off the request path.** State-change transactions
write to `outbox_events` in the same transaction. A background worker
(tokio task, in-process for the MVP) reads outbox rows, expands them to
one `webhook_deliveries` row per active endpoint, and POSTs. The API
returns to the caller before any HTTP egress happens, so the worst-case
caller latency is bounded by our database, not by the slowest webhook
receiver. The outbox-then-fan-out structure also handles the
"endpoint registered between state-change and dispatch" case correctly

- we look up endpoints at dispatch time, not at write time.

In production this worker would be a separate process. In-process is the
right MVP choice because it doesn't fork the deploy story; a separate
binary is a trivial later split since the worker has no shared state.

## 5. API Key Model

**Generation.** 32 random bytes from `OsRng`, base64url-encoded, prefixed
`dodo_sk_live_`. Total length is around 56 characters.

**Storage.** Only the SHA-256 hash is persisted in `api_keys.key_hash`.
Plaintext is shown ONCE on creation. A short prefix (`dodo_sk_live_abcd1234`)
is stored separately for dashboard listing and audit logs - operators can
identify which key was used without ever needing the plaintext.

SHA-256 over bcrypt/argon2 because (a) keys are 32 bytes of entropy from a
CSPRNG, not user-chosen passwords, so we don't need slow KDFs to resist
dictionary attacks; (b) auth happens on every request and we don't want a
~100 ms hash on the hot path. We accept the trade-off that a DB dump plus a
GPU attacker would not be slowed down - but the unique index makes a
dictionary-on-hashes pointless against random 32-byte keys (2^256 search
space).

**Transmission.** `Authorization: Bearer dodo_sk_live_…`. TLS termination at
the load balancer in production; out of scope for the assignment.

**Rotation.** Multiple active keys per business. The create-key endpoint
returns the new plaintext; the caller flips traffic; the operator revokes
the old key. There's no atomic "rotate" operation because there can't be -
the caller needs the new key in hand before invalidating the old.

**Revocation.** Set `revoked_at`; auth middleware checks the column on every
request. The same query that resolves `key_hash - business_id` includes
`revoked_at IS NULL`, so revocation is one UPDATE and takes effect on the
next request.

**Blast radius if leaked.** A leaked key has full business scope: read and
mutate every customer, invoice, payment attempt, and webhook endpoint owned
by that business. Mitigations not built in this MVP: per-key scoping (read
vs. write, customers-only vs. payments-allowed), per-key rate limits, and IP
allowlists. All discussed below in §6.

## 6. What I Cut and Why

I built only the must-haves. Things I considered and deliberately did not
build:

1. **Per-key scoping / least-privilege API keys.** Every key has full
   business scope. The right shape is a `scopes` column on `api_keys`
   plus middleware that checks per-route. Cut because the spec ask is
   "API key authentication", not RBAC, and rolling out scopes correctly
   needs a real policy decision (which scopes? defaults?) that doesn't
   belong in a take-home.

2. **Event-history endpoint (`GET /v1/events?since=…`).** Mentioned in
   the webhook reconciliation discussion. Cut for time. The `outbox_events`
   table is already the source of truth - exposing it is mostly a paging
   exercise.

3. **Refunds / partial payments.** Out of scope per spec. The data model
   could support them as a separate `refunds` table with a foreign key
   to `payment_attempts`; the invoice state machine would gain a
   `refunded` state (or `paid_with_refund`). Not built.

4. **Rate limiting.** Real production needs per-business and per-IP
   limits. The right shape is a token-bucket in Redis keyed by business
   id, with limits configured per endpoint. Cut because Postgres-only
   rate limiting either contends with the hot path or is structurally
   advisory (sliding-window summaries). Discussed in §7.

5. **Observability beyond `tracing` logs.** No structured request IDs
   propagated through subsystems, no Prometheus metrics, no
   OpenTelemetry. We log key state transitions and PSP outcomes, which
   is enough to debug the happy path and the failure modes. Production
   needs at least: per-request IDs in headers, metrics on PSP latency
   and webhook-delivery success rate, and traces across the
   `/pay` - PSP - reconciler boundary.

## 7. Production Readiness Gap

If this shipped tomorrow, the top three things I'd insist on before declaring
done:

1. **Observability.** Per-request IDs propagated end-to-end (currently
   we synthesise a fresh one in each error response, which is wrong).
   Metrics for: PSP latency p50/p95/p99, count of pending attempts older
   than N seconds (this should alert if the reconciler stops), webhook
   delivery success rate per endpoint. Without these we can't tell the
   difference between "everything is fine" and "the reconciler died
   three hours ago and the dashboard says paid invoices have stopped
   appearing".

2. **Rate limiting.** Per-business token bucket on `/pay` is the most
   important - abuse there directly costs us (and the merchant) PSP
   fees. Without it a runaway client or compromised key can drive PSP
   costs unbounded.

3. **Audit log.** Right now we have row history but no record of _who_
   triggered each transition. For a payments system, that's table-stakes
   - operators need to answer "who voided invoice X?" months later. The
     shape: an append-only `audit_log` table with `(business_id, api_key_id,
action, target_id, payload_summary, ts)`, written in the same
     transaction as the action. We have all the pieces; we just don't
     write the row.
