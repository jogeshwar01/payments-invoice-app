# Architecture And Request Flow

## Components

### API service

Rust Actix-web binary: `dodo-api`.

Responsibilities:

- authenticate API keys;
- handle customer/invoice/payment/webhook endpoint routes;
- run migrations on startup;
- spawn in-process background workers for webhook dispatch and payment reconciliation.

Key files:

- `crates/api/src/main.rs`
- `crates/api/src/lib.rs`
- `crates/api/src/routes/*`

### Postgres

System of record.

Responsibilities:

- tenant data;
- invoice state;
- payment attempts and idempotency keys;
- webhook outbox and delivery retry state;
- database constraints that reinforce app-level rules.

### Mock PSP

Rust Actix-web binary: `mock-psp`.

Responsibilities:

- emulate payment outcomes by `card_token`;
- store outcomes by idempotency key;
- expose lookup endpoint for reconciliation;
- expose debug endpoints used by tests.

Important tokens:

- `tok_success`: succeeds after about 100 ms.
- `tok_insufficient_funds`: fails after about 100 ms.
- `tok_card_declined`: fails after about 100 ms.
- `tok_timeout`: sleeps 30 seconds, then succeeds.
- `tok_network_error`: returns 500 and does not record an outcome.

### Background workers

Both are in-process Tokio tasks in the MVP.

- `reconciler`: scans old pending payment attempts and asks PSP lookup for the result.
- `dispatcher`: fans out outbox events and sends signed webhooks with retry.

Production answer:

> "I kept them in-process for deploy simplicity in the take-home. The data model already lets them become separate binaries because all state lives in Postgres."

## Payment request flow

`POST /v1/invoices/{id}/pay`

```text
client
  -> Actix route
  -> BusinessCtx extractor validates API key
  -> IdempotencyKey extractor validates header
  -> Tx A:
       SELECT invoice FOR UPDATE
       read existing payment_attempt by (business_id, idempotency_key)
       reject different request body
       replay existing completed/pending attempt
       require invoice state == open
       insert pending payment_attempt
       update invoice open -> processing
       commit
  -> PSP HTTP call with 5s timeout
  -> Tx B if result is known:
       SELECT invoice FOR UPDATE
       SELECT payment_attempt FOR UPDATE
       no-op if already final
       update attempt succeeded/failed
       update invoice processing -> paid/open
       insert outbox event
       commit
  -> response
```

Why two transactions:

- Holding a row lock while calling the PSP would tie database availability to network latency.
- The `processing` state preserves correctness after Tx A commits.
- Other requests can observe "payment in flight" without waiting on a long external call.

## Webhook request flow

State-changing transaction:

```text
update invoice/payment state
insert outbox_events row
commit
return API response
```

Dispatcher:

```text
select undispatched outbox_events
lock one event
read active endpoints
insert webhook_deliveries rows
mark outbox dispatched

select due webhook_deliveries
claim row by conditional UPDATE
load endpoint secret and event payload
sign body
POST to receiver
mark delivered or schedule retry
```

Why outbox:

- The event write is atomic with the database state change.
- API latency does not depend on receiver latency.
- If the process crashes after commit, the outbox row is still there.
- Retries are durable.

## Data ownership

Everything tenant-owned has `business_id` either directly or through a parent.

Main tables:

- `businesses`: auth boundary.
- `api_keys`: credentials for a business.
- `customers`: customer records scoped to a business.
- `invoices`: invoice header and state.
- `invoice_line_items`: invoice detail rows.
- `payment_attempts`: every payment try, with idempotency key and request hash.
- `webhook_endpoints`: receiver URLs and signing secrets.
- `outbox_events`: durable event stream.
- `webhook_deliveries`: per-endpoint retry state.

## Why Actix-web

Good answer:

> "Actix is mature, fast, and has a clean extractor model. I used extractors for `BusinessCtx` and `IdempotencyKey`, which keeps handler signatures explicit: if a route needs auth or idempotency, the type appears in the function arguments."

## Why sqlx runtime queries

Good answer:

> "I used sqlx for async Postgres access and migrations. This repo uses runtime query APIs rather than compile-time checked queries so the build does not require a running database. The tradeoff is less compile-time SQL validation, so integration tests against real Postgres matter more."

## Why mock PSP is a separate service

Good answer:

> "The assignment says to treat the PSP as a real external dependency. A separate service forces HTTP, timeout behavior, network error handling, and docker-compose orchestration. If it were only a local function call, the hardest failure modes would be fake."

## 100x scale answer

At higher traffic:

- Split dispatcher and reconciler into separate worker deployments.
- Add leader election or row-claiming patterns everywhere workers can run concurrently.
- Partition webhook delivery and outbox tables by time.
- Add metrics and alerting on pending attempts, webhook failure rate, PSP latency, and lock waits.
- Add Redis or a dedicated limiter for per-business and per-IP rate limiting.
- Add read replicas for list/read endpoints, while keeping payment writes on primary.
- Consider sharding by `business_id` only after exhausting simpler options.

