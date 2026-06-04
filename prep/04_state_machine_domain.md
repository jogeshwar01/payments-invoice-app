# State Machine And Domain Model

## States

```text
draft
  -> open
  -> void

open
  -> processing
  -> void
  -> uncollectible

processing
  -> paid
  -> open

paid, void, uncollectible
  -> terminal
```

## State meanings

### draft

Invoice exists but is not yet payable.

Why keep it:

- It mirrors real billing systems where line items can be prepared before finalization.
- It makes finalization an explicit business action.
- Demo uses `?finalize=true` to reduce curl steps.

### open

Invoice is finalized and can accept payment.

Important:

- Only `open` invoices can go to `processing`.

### processing

Payment is in flight.

This is the most important state.

Why it exists:

- It survives after Tx A commits and before PSP result.
- It blocks another `/pay` with a different key.
- It gives callers a truthful status when PSP result is unknown.

Interview phrase:

> "`processing` is not cosmetic. It is the concurrency guard."

### paid

Terminal successful payment state.

No outgoing transitions.

Refunds would be separate records, not a mutation back to `open`.

### void

Terminal cancellation before payment.

No outgoing transitions.

### uncollectible

Terminal "we do not expect to collect" state.

The code includes the transition in the pure state machine, although there is no public route for it.

## Transition table

| From | Event | To | Trigger |
|---|---|---|---|
| `draft` | `Finalize` | `open` | `POST /v1/invoices/{id}/finalize` |
| `draft` | `Void` | `void` | `POST /v1/invoices/{id}/void` |
| `open` | `Pay` | `processing` | Tx A of `/pay` |
| `open` | `Void` | `void` | `POST /v1/invoices/{id}/void` |
| `open` | `MarkUncollectible` | `uncollectible` | future admin op |
| `processing` | `PspSuccess` | `paid` | Tx B/reconciler |
| `processing` | `PspFailure` | `open` | Tx B/reconciler |

## Why terminal states are terminal

Payments systems prefer append-only correction records over rewriting history.

Examples:

- If a paid invoice is refunded, create a `refunds` row.
- If a void was accidental, create a new invoice or an audit-correcting action.
- If an invoice is uncollectible but later paid, production design would need a deliberate re-open flow with audit log and permissions.

## Invalid transitions

Implemented in `try_transition`.

Why a pure function:

- Easy to unit test.
- All routes use the same rules.
- Avoids scattered `if state == ...` checks.
- Keeps the state diagram and code close.

Database reinforcement:

- `invoices.state` has a `CHECK` constraint for allowed state strings.
- The DB does not enforce the transition graph; app code does.

If asked why not database triggers:

> "Triggers could enforce transitions, but they make application behavior less visible and harder to test in Rust. I used DB constraints for shape invariants and application code for domain transitions."

## Why not just a boolean `paid`

A boolean cannot represent:

- draft vs open;
- payment in flight;
- void;
- uncollectible;
- terminal vs retryable failure.

The `processing` state especially cannot be modeled safely by just `paid=false`.

## Why PSP failure goes back to `open`

For known failures like `card_declined`:

- The PSP definitively did not charge.
- The user should be able to retry with a different card.
- The attempt remains recorded as `failed`.
- The invoice returns to payable state.

For timeout/network ambiguity:

- The code leaves `processing` and `pending` because outcome is unknown.

## Common question: what about partial payments

Out of scope.

Production design:

- Add `payments` or `charges` ledger table.
- Track amount paid and amount remaining.
- Invoice state might include `partially_paid`.
- Double-spend logic moves from invoice-level "one charge" to amount-level accounting.
- Idempotency still belongs on each payment attempt.

Good answer:

> "I deliberately did not add partial payments because it complicates the state machine and money ledger. The assignment explicitly asked for minimal invoices and payment attempts."

## Common question: what about refunds

Out of scope.

Production design:

- Add `refunds(payment_attempt_id, amount_cents, status, psp_ref, idempotency_key)`.
- Do not move invoice from `paid` back to `open`.
- Expose refund status separately.
- Emit `refund.created` / `refund.succeeded` webhooks.

## Common question: where do due dates matter

Current MVP:

- Stored on invoices.
- No automated overdue job.

Production:

- A scheduled job could transition old `open` invoices to `uncollectible` or create dunning attempts.
- That was cut because subscriptions/dunning are out of scope.

