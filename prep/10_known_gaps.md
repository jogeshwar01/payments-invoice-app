# Known Gaps And How To Discuss Them

Interviewers often probe gaps to see whether you understand your own system. Do not hide these. State the issue, explain why it was acceptable for the assignment, and give the production fix.

## 1. Unauthenticated additional API key creation

Current:

- `POST /v1/businesses` is intentionally unauthenticated bootstrap.
- `POST /v1/businesses/{id}/api_keys` is also unauthenticated.

Risk:

- Anyone who knows or guesses a business ID could create a valid API key.

Best answer:

> "This should not ship. The extra key creation route was a demo/key-rotation shape, but it needs to be an authenticated admin/console operation. For the assignment, the required auth path is the bearer key middleware on business resources. The production fix is to remove that route from public API or require an existing key with key-management permission."

## 2. `tok_network_error` can remain pending forever

Current:

- Network error returns `202 pending`.
- Reconciler looks up PSP by `attempt_id`.
- Mock `tok_network_error` returns 500 and does not store an outcome.
- Lookup therefore returns 404 forever.

Why the conservative choice is reasonable:

- Real network errors are ambiguous.
- The PSP may have charged but response was lost.
- Reopening invoice immediately can double charge.

Production fix:

- Classify errors:
  - definitive processor decline/failure -> mark failed and reopen;
  - ambiguous transport error -> pending/manual review.
- Add max pending age and alerting.
- Add operator resolution endpoint.
- Use PSP settlement reports/webhooks for final reconciliation.

## 3. Docs mention `OsRng`, code uses `thread_rng`

Current:

- `DESIGN.md` says 32 random bytes from `OsRng`.
- `auth.rs` and webhook endpoint creation use `rand::thread_rng().fill_bytes`.

How to explain:

> "`ThreadRng` in rand is a CSPRNG seeded from the OS, so the key entropy property is still fine. But the docs should match code. I would either switch to `OsRng` for explicitness or update the docs to say CSPRNG."

## 4. Request IDs are synthesized only in error responses

Current:

- Error JSON includes `request_id`.
- There is no middleware-generated request ID propagated through logs, PSP calls, or webhooks.

Production fix:

- Add request-id middleware.
- Accept incoming `X-Request-Id` or generate one.
- Put it in tracing spans, responses, and downstream calls.

## 5. Webhook URL validation is weak

Current:

- Accepts any string starting with `http://` or `https://`.

Risk:

- SSRF to internal metadata services or private network.

Production fix:

- Parse URL properly.
- Resolve DNS and block private/link-local/loopback IPs.
- Restrict redirects.
- Use egress proxy or allowlist.
- Add per-endpoint timeout and body limits.

## 6. Webhook delivery is not fully tested

Current:

- Signing unit tests exist.
- No integration test for retry schedule or receiver failures.

Production/test fix:

- Run a local fake receiver that fails first N attempts then succeeds.
- Assert signature header.
- Assert retry count and final delivered status.
- Assert failed status after max attempts.

## 7. Crash-recovery path is reasoned, not directly tested

Current:

- Reconciler test covers `tok_timeout`.
- It does not directly simulate a crash after PSP success before Tx B.

Production/test fix:

- Add a test hook or fault injection point between PSP response and Tx B.
- Kill/restart API process.
- Assert reconciler marks attempt succeeded without a second PSP charge.

## 8. Idempotency test does not directly assert PSP call count

Current:

- It asserts same `psp_ref` on replay.
- Helper has `psp_call_count`, but the test does not use it.

Production/test fix:

- Reset PSP debug state.
- Assert call count increments once after repeated same-key calls.

## 9. No pagination beyond `LIMIT 100`

Current:

- List endpoints return recent 100.

Production fix:

- Cursor pagination by `(created_at, id)`.
- Stable ordering.
- Page size limits.

## 10. No audit log

Current:

- Row state exists, but no append-only "who did what" history.

Production fix:

- `audit_log(business_id, api_key_id, action, target_type, target_id, metadata, created_at)`.
- Write in the same transaction as state-changing action.

## 11. No rate limiting

Current:

- No per-business or per-IP limiter.

Risk:

- Compromised key or buggy client can hammer `/pay`.
- Could create PSP cost or abuse.

Production fix:

- Redis token bucket by business/key/IP.
- Stricter limits on payment endpoints.
- Alert on anomalous volume.

## 12. No event replay endpoint

Current:

- `outbox_events` stores events.
- API does not expose `GET /v1/events`.

Production fix:

- Cursor-paginated events endpoint.
- Filter by type and created time.
- Manual webhook replay by event ID.

## 13. Idempotency unique-violation fallback should validate more

Current:

- Idempotency keys are unique per business.
- The normal path checks `request_hash` before replaying an existing attempt.
- The `23505` unique-violation fallback re-reads the existing attempt but does not re-check `request_hash` or selected `invoice_id`.

Why this mostly works in the common path:

- For two concurrent requests against the same invoice, the invoice row lock serializes them, so the second request usually sees the existing attempt through the normal hash-check path.

Edge case:

- Two different invoices in the same business use the same idempotency key concurrently.
- They lock different invoice rows, so both can race to insert.
- One insert wins; the other hits `23505`.
- The fallback should detect "same key but different request/resource" and return `409 idempotency_key_conflict`.

Production fix:

- Make `read_existing` also load `invoice_id` and `request_hash`.
- Return conflict if either does not match the incoming request.
- Alternatively scope idempotency uniqueness to `(business_id, endpoint, idempotency_key)` or `(business_id, invoice_id, idempotency_key)`, but that changes API semantics and must be documented.

## 14. Webhook endpoint snapshot semantics are not explicit

Current:

- `outbox_events` stores the event.
- Dispatcher later reads currently active webhook endpoints and creates deliveries.

Edge case:

- An endpoint registered after an event is committed but before dispatcher fan-out may receive that earlier event.
- An endpoint deleted before fan-out may not receive an event that happened while it was active.

Production fix:

- Decide semantics explicitly.
- If "registered endpoints at event time" is required, materialize delivery rows inside the state-change transaction or include endpoint creation timestamps and filter `endpoint.created_at <= event.created_at`.
- If "active endpoints at dispatch time" is intended, document it.

## 15. Zero-total invoice behavior is not a product decision

Current:

- Quantity must be positive.
- Unit amount can be zero.
- Total can therefore be zero.
- A zero-total open invoice can go through `/pay` with amount 0.

Production fix:

- Decide product behavior:
  - reject zero-total invoices;
  - allow them but auto-mark paid on finalization;
  - allow zero-price invoices for trials/credits without PSP call.
- Encode that rule in both app validation and tests.

## How to frame gaps

Good pattern:

```text
Yes, that is a gap.
The assignment version does X because Y.
The risk is Z.
The production fix is A/B/C.
```

Avoid:

- "I did not have time" as the only answer.
- Pretending a gap is not a gap.
- Overbuilding a speculative fix verbally without naming the tradeoff.
