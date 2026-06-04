# Postgres Locking And Data Model

## Data model spine

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

All main resources are scoped by `business_id`.

## Key constraints

### API keys

```sql
CREATE UNIQUE INDEX api_keys_key_hash_idx ON api_keys (key_hash);
```

Purpose:

- Indexed auth lookup.
- Prevent duplicate key hashes.

### Invoice state

```sql
state TEXT NOT NULL CHECK (state IN (...))
```

Purpose:

- Prevent invalid state strings.
- App code still enforces allowed transitions.

### Money

```sql
total_cents BIGINT NOT NULL CHECK (total_cents >= 0)
unit_amount_cents BIGINT NOT NULL CHECK (unit_amount_cents >= 0)
quantity INTEGER NOT NULL CHECK (quantity > 0)
```

Purpose:

- Never store floats.
- Prevent negative line items in DB even if app validation regresses.

### Payment idempotency

```sql
CREATE UNIQUE INDEX payment_attempts_idem_idx
ON payment_attempts (business_id, idempotency_key);
```

Purpose:

- Database-enforced idempotency.
- Safe under concurrency across API instances.

### Pending attempt index

```sql
CREATE INDEX payment_attempts_pending_idx
ON payment_attempts (created_at)
WHERE status = 'pending';
```

Purpose:

- Reconciler scans old pending rows efficiently.

### Outbox and delivery indexes

Partial indexes on:

- `outbox_events(created_at) WHERE dispatched_at IS NULL`
- `webhook_deliveries(next_attempt_at) WHERE status='pending'`

Purpose:

- Workers scan only actionable rows.

## `SELECT ... FOR UPDATE`

Used on invoice rows in payment flow and simple state transitions.

What it does:

- Locks selected rows until transaction commit/rollback.
- Other transactions trying to lock the same row block.
- Reads without `FOR UPDATE` can still read old committed state depending on isolation level.

In this service:

- The invoice row is the concurrency boundary for payment.
- Lock is held only around DB work.
- PSP call happens after commit.

Good answer:

> "The row lock serializes only operations that mutate the same invoice. It does not block unrelated invoices."

## Default isolation level

Postgres default is `READ COMMITTED`.

Why it is enough here:

- We explicitly lock the row we depend on before checking state.
- The second transaction sees the committed state after the first transaction releases the lock.
- Idempotency has a unique constraint, which is safe under concurrent inserts.

If asked why not serializable:

> "Serializable would also work, but I would then need a retry loop for serialization failures. Since the conflict is exactly one invoice row, row locking is simpler and more explicit."

## Deadlock risk

Low in the current code because:

- Payment Tx A locks only one invoice row, then inserts attempt.
- Tx B locks invoice then attempt consistently.
- Simple transitions lock only one invoice.
- Dispatcher claims rows with conditional updates and event locks.

Potential issue:

- If future code locks attempt first and invoice second, it could deadlock with Tx B.

Production rule:

- Always lock invoice before payment attempts for payment-related operations.
- Keep transaction sections small.
- Add lock wait metrics.

## Why not hold a DB lock during PSP call

Problems:

- PSP timeout is 30 seconds for `tok_timeout`.
- Holding a lock for that long causes other invoice operations to queue.
- Holding a DB connection for network I/O can exhaust the pool.
- If many PSP calls are slow, the whole API could starve for connections.

The `processing` state solves this:

- Claim with Tx A.
- Release lock.
- Do network I/O.
- Finish with Tx B.

## Idempotency race with same key

Even if two requests with the same idempotency key both reach insert:

- Unique index allows only one row.
- Loser receives SQLSTATE `23505`.
- Code catches it and re-reads the existing attempt.

Important:

> "Idempotency is not an in-memory cache. It is durable and works across multiple API processes."

## Money handling

Why integer cents:

- Floating-point cannot exactly represent decimal money.
- Summing floats can produce rounding surprises.
- Cents as `i64` is simple for single-currency USD.

Why `BIGINT`:

- Enough range for realistic invoice totals.
- Matches Rust `i64`.

Why checked arithmetic:

- Prevent overflow when multiplying `quantity * unit_amount_cents`.
- Reject bad inputs before storing.

If asked about decimals:

> "For single-currency cents, integers are simpler and safer. For multi-currency with different minor units, I would store integer minor units plus currency metadata, not floats."

## Indexing choices

`customers(business_id, email)`:

- Useful for tenant-scoped lookup.

`invoices(business_id, state, created_at DESC)`:

- Serves `GET /invoices?state=open`.

`invoice_line_items(invoice_id, position)`:

- Efficiently loads line items in display order.

`payment_attempts(invoice_id, created_at DESC)`:

- Fetch payment history per invoice.

Partial pending indexes:

- Keep worker scans small.

## What changes at scale

Near-term:

- Add pagination instead of `LIMIT 100`.
- Add query timeouts.
- Add metrics around lock waits and slow queries.
- Move workers out of API process.

Larger scale:

- Partition outbox and deliveries by time.
- Archive old payment attempts and deliveries.
- Separate OLTP tables from analytics.
- Add read replicas for list endpoints.
- Shard by `business_id` only if single-primary Postgres becomes the bottleneck.

