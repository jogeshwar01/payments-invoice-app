# Database Schema Walkthrough

Use this when someone asks: "Explain your database schema."

## High-level shape

```text
businesses
  -> api_keys
  -> customers
       -> invoices
            -> invoice_line_items
            -> payment_attempts
  -> webhook_endpoints
  -> outbox_events
       -> webhook_deliveries
```

`businesses` is the tenant boundary. Almost every table either has `business_id` directly or is reachable through a parent row that has it.

## Schema by table

## `businesses`

Purpose:

- Top-level tenant/account.
- API keys, customers, invoices, webhooks, and events belong to a business.

Important columns:

- `id UUID PRIMARY KEY`
- `name TEXT NOT NULL`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`

Why UUID:

- Easy to generate in application code.
- Avoids exposing sequential IDs.
- Works cleanly across services/tests.

## `api_keys`

Purpose:

- Stores API credentials for businesses.
- Used by auth middleware to resolve a bearer token to `business_id`.

Important columns:

- `id UUID PRIMARY KEY`
- `business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE`
- `key_hash BYTEA NOT NULL`
- `key_prefix TEXT NOT NULL`
- `name TEXT NOT NULL DEFAULT ''`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `revoked_at TIMESTAMPTZ NULL`

Indexes:

- `UNIQUE(key_hash)`
- index on `business_id`

Design answer:

> "I store only a SHA-256 hash of the API key, not plaintext. The prefix is stored only for display/debugging. `revoked_at` lets revocation be a single update."

Important gap:

- The column supports revocation, but a public revoke route is not implemented.

## `customers`

Purpose:

- Customer records owned by one business.

Important columns:

- `id UUID PRIMARY KEY`
- `business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE`
- `name TEXT NOT NULL`
- `email TEXT NOT NULL`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`

Indexes:

- `(business_id, email)`

Design answer:

> "Customers are tenant-scoped. I indexed `(business_id, email)` because list/search by customer email is a common tenant-local lookup. Email is not globally unique."

## `invoices`

Purpose:

- Invoice header/state row.
- This is the row locked during payment.

Important columns:

- `id UUID PRIMARY KEY`
- `business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE`
- `customer_id UUID NOT NULL REFERENCES customers(id)`
- `state TEXT NOT NULL CHECK (...)`
- `total_cents BIGINT NOT NULL CHECK (total_cents >= 0)`
- `currency TEXT NOT NULL DEFAULT 'USD'`
- `due_date DATE NULL`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `updated_at TIMESTAMPTZ NOT NULL DEFAULT now()`

Allowed states:

- `draft`
- `open`
- `processing`
- `paid`
- `void`
- `uncollectible`

Indexes:

- `(business_id, state, created_at DESC)`
- `(customer_id)`

Design answer:

> "The invoice row is the concurrency boundary. `SELECT ... FOR UPDATE` on this row serializes payment attempts for the same invoice. `total_cents` is computed by the server from line items and stored for fast reads."

Why `CHECK` on state:

- Prevents invalid state strings even if app code regresses.
- Transition graph is still enforced in Rust by `try_transition`.

## `invoice_line_items`

Purpose:

- Stores invoice detail rows used to compute totals.

Important columns:

- `id UUID PRIMARY KEY`
- `invoice_id UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE`
- `description TEXT NOT NULL`
- `quantity INTEGER NOT NULL CHECK (quantity > 0)`
- `unit_amount_cents BIGINT NOT NULL CHECK (unit_amount_cents >= 0)`
- `position INTEGER NOT NULL`

Indexes:

- `(invoice_id, position)`

Design answer:

> "The client sends line items, not a total. The service computes `sum(quantity * unit_amount_cents)` using checked integer arithmetic, then stores the result on `invoices.total_cents`."

Why no floats:

- Money is stored in integer cents.
- Single currency USD means no FX/minor-unit complexity.

## `payment_attempts`

Purpose:

- One row per `/pay` attempt.
- Stores idempotency, PSP result, and failure/success status.

Important columns:

- `id UUID PRIMARY KEY`
- `invoice_id UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE`
- `business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE`
- `idempotency_key TEXT NOT NULL`
- `request_hash BYTEA NOT NULL`
- `status TEXT NOT NULL CHECK (status IN ('pending','succeeded','failed'))`
- `psp_ref TEXT NULL`
- `failure_code TEXT NULL`
- `response_json JSONB NULL`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `completed_at TIMESTAMPTZ NULL`

Indexes:

- `UNIQUE(business_id, idempotency_key)`
- `(invoice_id, created_at DESC)`
- partial index on `(created_at) WHERE status = 'pending'`

Design answer:

> "`UNIQUE(business_id, idempotency_key)` is the idempotency primitive. `request_hash` prevents using the same idempotency key for a different request body. The pending partial index lets the reconciler find stale attempts efficiently."

Statuses:

- `pending`: PSP outcome is unknown or in progress.
- `succeeded`: PSP charged successfully.
- `failed`: known PSP decline/failure.

Important nuance:

- Idempotency is business-scoped in this implementation. A stricter production fallback should re-check `invoice_id` and `request_hash` after unique-violation races.

## `webhook_endpoints`

Purpose:

- Stores receiver URLs and signing secrets for each business.

Important columns:

- `id UUID PRIMARY KEY`
- `business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE`
- `url TEXT NOT NULL`
- `signing_secret BYTEA NOT NULL`
- `active BOOLEAN NOT NULL DEFAULT TRUE`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`

Indexes:

- partial index on `business_id WHERE active`

Design answer:

> "Each endpoint has its own HMAC secret. The full secret is returned once on creation. Later list calls return only a prefix."

Security gap:

- Current URL validation only checks `http://` or `https://`.
- Production needs SSRF protection.

## `outbox_events`

Purpose:

- Durable event stream written in the same transaction as invoice/payment state changes.

Important columns:

- `id UUID PRIMARY KEY`
- `business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE`
- `event_type TEXT NOT NULL`
- `payload JSONB NOT NULL`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `dispatched_at TIMESTAMPTZ NULL`

Indexes:

- partial index on `(created_at) WHERE dispatched_at IS NULL`

Design answer:

> "This is the outbox pattern. If the invoice state change commits, the webhook event commits with it. The API does not call webhook receivers inline."

Events emitted:

- `invoice.created`
- `invoice.paid`
- `invoice.payment_failed`

## `webhook_deliveries`

Purpose:

- Per-endpoint delivery attempts for each outbox event.
- Stores retry state.

Important columns:

- `id UUID PRIMARY KEY`
- `outbox_event_id UUID NOT NULL REFERENCES outbox_events(id) ON DELETE CASCADE`
- `webhook_endpoint_id UUID NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE`
- `attempt_count INTEGER NOT NULL DEFAULT 0`
- `next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `last_status_code INTEGER NULL`
- `last_error TEXT NULL`
- `status TEXT NOT NULL CHECK (status IN ('pending','delivered','failed'))`
- `delivered_at TIMESTAMPTZ NULL`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`

Indexes:

- partial index on `(next_attempt_at) WHERE status = 'pending'`

Design answer:

> "I separated `outbox_events` from `webhook_deliveries` so one business event can fan out to multiple endpoints, each with independent retry state."

Retry schedule:

- 30 seconds
- 2 minutes
- 10 minutes
- 1 hour
- 6 hours
- 24 hours

After retry exhaustion:

- Delivery becomes `failed`.
- Event remains available for future reconciliation/manual replay design.

## Why JSONB for event payloads and response JSON

`outbox_events.payload`:

- Event payloads differ by event type.
- JSONB makes adding new event fields cheap.
- The stable contract is event type + payload shape.

`payment_attempts.response_json`:

- Stores PSP-mapped result for debugging/replay.
- Main query fields still live in typed columns (`status`, `psp_ref`, `failure_code`).

Good answer:

> "I use typed columns for query-critical fields and JSONB for variable payload snapshots."

## Why duplicate `business_id` on `payment_attempts`

Even though `payment_attempts` can reach business through invoice, it stores `business_id` directly.

Why:

- Makes idempotency uniqueness simple: `(business_id, idempotency_key)`.
- Makes auth-scoped queries cheaper.
- Avoids a join in the hot idempotency path.

Tradeoff:

- Denormalized column must stay consistent.
- Foreign keys to both invoice and business do not guarantee the invoice belongs to that same business unless an additional composite FK is added.

Production hardening:

- Add composite unique key on `invoices(id, business_id)`.
- Add composite FK from `payment_attempts(invoice_id, business_id)` to `invoices(id, business_id)`.

## Schema invariants

The database enforces:

- valid state strings;
- valid payment statuses;
- non-negative money;
- positive quantities;
- unique idempotency keys per business;
- FK ownership references.

The application enforces:

- valid state transitions;
- server-computed invoice totals;
- tenant-scoped access;
- no payment except from `open`;
- idempotency request-body matching.

Good summary:

> "The database enforces shape invariants. The Rust domain layer enforces workflow invariants."

## If asked what you would change first

Top schema improvements:

1. Add composite foreign keys to guarantee denormalized `business_id` consistency.
2. Add audit log table.
3. Add event replay cursor/index design.
4. Add API key scopes table or scopes column.
5. Add explicit stuck-payment resolution table/status.
6. Add pagination-friendly compound indexes for list endpoints.

