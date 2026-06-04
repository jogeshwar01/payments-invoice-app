# Testing, Demo, And Code Review Prep

## Required tests

### Concurrency test

File: `crates/api/tests/concurrency.rs`

What it does:

- Creates one invoice.
- Fires 20 concurrent `/pay` requests with distinct idempotency keys.
- Asserts exactly one succeeds.
- Asserts final invoice state is `paid`.

What it proves:

- Row locking and `processing` prevent multiple PSP-success paths for the same invoice.

What it does not fully prove:

- It does not query DB for exact count of successful attempts.
- It does not prove behavior across multiple API processes, although DB primitives should hold.

If asked:

> "The strongest possible version would also assert exactly one `payment_attempts.status='succeeded'` row and PSP call count. The current test checks the observable API result and final state."

### Idempotency test

File: `crates/api/tests/idempotency.rs`

What it does:

- Pays with one key.
- Repeats same key and same body.
- Asserts same attempt ID and same PSP ref.
- Reuses same key with a different card token and expects 409.

What it proves:

- Same-key replay returns cached response.
- Different-body reuse is rejected.

Subtle note:

- It infers one PSP call from same `psp_ref`. The helper has a `psp_call_count` method but the test does not call it.

### PSP failure test

File: `crates/api/tests/psp_failure.rs`

What it does:

- Uses `tok_timeout`.
- Asserts endpoint returns before 10 seconds.
- Asserts response is `202 pending`.
- Asserts invoice is initially `processing`.
- Polls until reconciler moves it to `paid`.
- Also checks paid invoice rejects further payment with 422.

What it proves:

- Timeout does not hang caller for 30 seconds.
- Reconciler can finish pending success.
- Terminal paid state blocks re-payment.

What it does not prove:

- `tok_network_error` recovery.
- Crash after PSP success before Tx B.
- Webhook delivery retries.

## Unit tests

State-machine tests in `invoice_state.rs` cover:

- happy path;
- pay only from open;
- terminal states have no outgoing transitions;
- PSP failure reverts processing to open.

Signer tests cover:

- deterministic signature for same body/timestamp;
- body change changes signature.

## How to demo

Recommended flow:

1. `docker compose down -v`
2. `docker compose up --build`
3. Bootstrap business.
4. Create customer.
5. Register webhook endpoint.
6. Create finalized invoice.
7. Show `total_cents` is computed.
8. Pay with `tok_success`.
9. Replay same idempotency key.
10. Create another invoice.
11. Pay with `tok_card_declined`.
12. Show invoice goes back to `open`.
13. Show webhook events.

## Demo talking points

When creating invoice:

> "The client sends line items only. The server computes the total in cents using checked integer arithmetic."

When paying:

> "This endpoint is the critical path. It requires `Idempotency-Key`. The handler claims the invoice in Tx A, calls PSP without a lock, and finishes in Tx B."

When replaying idempotency:

> "Same key and same body returns the same attempt. A new key on a paid invoice is rejected because `paid` is terminal."

When showing webhooks:

> "The API did not wait for this receiver. It wrote an outbox row, and the background dispatcher delivered it with HMAC signature."

## Code review challenges and answers

### Why does `create_api_key` not require auth?

Honest answer:

> "That endpoint is an MVP/demo shortcut and should not ship. The assignment only needed bootstrap API key auth. In production, key creation must be an authenticated console/admin operation, or the route should be removed."

### Why does network error leave pending?

Honest answer:

> "Because network ambiguity is dangerous in payments. If the PSP saw the request but the response was lost, reopening the invoice could allow a second charge. The gap is that the mock's `tok_network_error` never records an outcome, so the attempt can remain pending forever. Production needs a final ambiguity policy: manual review, expiry to failed after PSP guarantee window, or settlement-file reconciliation."

### Why no webhook tests?

Answer:

> "Given the time budget I focused tests on the three graded correctness paths. I did include signer unit tests. The next test I would add is a fake receiver that fails twice then succeeds, asserting retry count, signature header, and final delivered state."

### Why no request-id middleware?

Answer:

> "The error format includes a request ID, but the MVP synthesizes it per error response. Production should generate/request-propagate one at middleware entry and include it in logs, response headers, and downstream PSP/webhook calls."

### Why runtime sqlx queries?

Answer:

> "I chose simpler local build ergonomics. The tradeoff is losing compile-time SQL validation. Integration tests against real Postgres mitigate some of that, but production code should consider sqlx offline metadata or stronger query checks in CI."

## Useful commands

Run stack:

```sh
docker compose up --build
```

Run test dependencies:

```sh
docker compose up -d postgres mock-psp
```

Run tests:

```sh
cargo test --workspace
```

Health:

```sh
curl -fsS http://localhost:7002/health
```

## If asked what you would test next

Priority order:

1. Crash recovery simulation after PSP success before Tx B.
2. Network error ambiguity and manual resolution/expiry.
3. Webhook retry and signature verification with a local fake receiver.
4. Cross-business isolation tests.
5. API key revocation test after implementing revoke route.
6. Concurrent same-key `/pay` test.
7. Payment attempt row count and PSP call count assertions in concurrency test.

