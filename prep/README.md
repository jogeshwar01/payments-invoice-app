# Interview Prep Index

Use this folder as the interview map for the Dodo Payments backend assignment. The goal is not to memorize every sentence. The goal is to be able to explain the system in your own words, defend the tradeoffs, and acknowledge gaps without sounding surprised.

## The 60-second system summary

This is a minimal invoice and payment service in Rust using Actix-web, sqlx, Postgres, and a separate mock PSP service. A business authenticates with API keys, creates customers and invoices, and calls `POST /v1/invoices/{id}/pay` with an idempotency key. The payment path uses two short database transactions with the external PSP call in between. Tx A locks the invoice row, creates a pending payment attempt, and moves the invoice from `open` to `processing`. The PSP call runs without holding a database lock. Tx B records the PSP outcome, moves the invoice to `paid` or back to `open`, and writes webhook events to an outbox. A reconciler resolves pending attempts after PSP timeouts or crashes. A dispatcher sends signed webhooks asynchronously with retries.

## Files in this prep folder

- [01_assignment_map.md](01_assignment_map.md): what the assignment asked for and how this repo answers it.
- [02_architecture_request_flow.md](02_architecture_request_flow.md): services, request flow, data ownership, background workers.
- [03_payment_correctness_idempotency.md](03_payment_correctness_idempotency.md): double-spend prevention, idempotency, PSP timeouts, crash recovery.
- [04_state_machine_domain.md](04_state_machine_domain.md): invoice states, transitions, terminal states, invalid transitions.
- [05_postgres_locking_data_model.md](05_postgres_locking_data_model.md): schema, indexes, row locks, alternatives, scaling.
- [06_webhooks_auth_security.md](06_webhooks_auth_security.md): webhook signing/retry/outbox plus API key model and security gaps.
- [07_rust_actix_sqlx.md](07_rust_actix_sqlx.md): Rust, async, Actix, sqlx, reqwest, error handling questions.
- [08_testing_demo_review.md](08_testing_demo_review.md): tests, demo talking points, likely code-review challenges.
- [09_question_bank.md](09_question_bank.md): rapid-fire interview questions with concise model answers.
- [10_known_gaps.md](10_known_gaps.md): honest gaps, doc mismatches, and how to discuss them.
- [11_database_schema_walkthrough.md](11_database_schema_walkthrough.md): dedicated table-by-table schema explanation.

## Highest-value source files to know

- `crates/api/src/routes/payments.rs`: core payment correctness path.
- `crates/api/src/domain/invoice_state.rs`: pure state machine.
- `crates/api/src/webhooks/reconciler.rs`: pending payment recovery.
- `crates/api/src/webhooks/dispatcher.rs`: outbox fan-out and signed delivery.
- `crates/api/src/auth.rs`: API key hashing and request extractors.
- `migrations/*.sql`: schema, indexes, constraints.
- `crates/mock-psp/src/main.rs`: PSP behavior by token.
- `crates/api/tests/*.rs`: required concurrency, idempotency, and PSP-failure tests.

## Answer style

Use this structure when answering:

1. State the mechanism first.
2. Explain the exact failure mode it protects.
3. Name the tradeoff.
4. If there is an MVP gap, say it plainly and give the production fix.

Example:

> "Concurrent payments are serialized by `SELECT ... FOR UPDATE` on the invoice row. The first request moves `open -> processing`; the second wakes up, sees `processing`, and returns `409 payment_in_progress` before calling the PSP. I chose this over serializable isolation because the hot row is obvious and the retry behavior is simpler. At higher scale I would add stronger observability around lock wait time and pending attempts."
