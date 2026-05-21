# AI_USAGE.md

## A note before the structured bits

Quick honest take before I get into the prescribed answers: I used AI heavily
on this one. I drove a Claude Code session in the terminal — explained what
the assignment was, talked through the design out loud, and had it produce
files while I steered. The architectural calls (two-transaction payment flow
with `processing` as an explicit guard state, using our `attempt_id` as the
PSP's idempotency key, the outbox-then-fan-out webhook split) are mine. The
typing is mostly Claude's.

The sections below are the ones the rubric explicitly asks for. I wrote the
intent for each; the prose was drafted with AI and edited by me. Where I say
"I rejected X" or "I caught Y" — those are real moments from the session,
not retrofitted narrative.

---

## 1. Which AI tools I used and for what

- **Claude (via Claude Code CLI)** — the main driver. Used for:
  - Drafting the initial sqlx migrations after I described the table list.
    I revised the `payment_attempts` index — Claude's first pass put a
    plain index on `(business_id, idempotency_key)`; I made it `UNIQUE`
    because that uniqueness is *the* idempotency primitive, not a
    nice-to-have.
  - Writing the Actix route handlers from a list I provided.
  - First-pass `DESIGN.md` prose. I supplied the structure, the failure-mode
    answers, and the reasoning; Claude turned bullets into paragraphs.
  - Doctest-block fixes when rustdoc tried to compile my ASCII diagrams as
    Rust.
- **No Cursor / Copilot / ChatGPT** on this one. Single agent, one
  conversation.

What I didn't use AI for: the state machine design, the two-transaction
flow, the choice to key the PSP by our attempt_id, the decision to skip
advisory locks, and the cut list in DESIGN.md §6. Those came out of thinking
about how Stripe handles `PaymentIntent` and what would break under
concurrency.

## 2. Three decisions I made myself, against or independent of AI

### 2.1. Two-transaction payment flow with `processing` as an explicit state

Claude initially proposed a single transaction wrapping the PSP HTTP call. I
rejected it because holding a Postgres row lock across a 5 s network call is
a foot-gun: the lock waiters back up, the connection pool fills, and the API
stops accepting unrelated requests.

I designed the two-transaction split: Tx A flips `open → processing` under a
row lock and **commits** before the PSP call; Tx B applies the result under
a fresh row lock after the call returns. The `processing` state is
load-bearing — it lets a concurrent `/pay` distinguish "another attempt is
in flight" from "this is paid" without holding any lock.

I also rejected an early Claude suggestion to use Postgres advisory locks.
Advisory locks share a 64-bit namespace across the whole database, are
invisible in pgAdmin, and require their own release discipline. A row lock
on `invoices` is self-documenting — anyone reading the code knows what's
being serialized.

### 2.2. Mock PSP keyed by *our* attempt_id, not by a PSP-generated id

The obvious shape — and what Claude wrote first — is: client POSTs to PSP,
PSP returns its own `psp_ref`, we store it. I changed it so we pass our
`attempt_id` as the *idempotency key* on the PSP side. The PSP stores the
outcome under that key.

This is what makes the crash-recovery story actually work. After a crash
between PSP-success and our Tx B, the reconciler can call
`GET /psp/charges/{attempt_id}` and recover the outcome — because the PSP
knows the key we used. Without this, recovery would require either (a)
re-issuing the charge (double charge!) or (b) some out-of-band
reconciliation the assignment doesn't ask for.

This is the same pattern Stripe uses for `PaymentIntent` IDs — they're our
identifier, not the processor's, and they're the join key for any later
reconciliation.

### 2.3. Server-computed invoice totals + integer-only money

Claude's first draft of the create-invoice handler accepted a `total_cents`
field in the request and validated that it matched the sum of line items. I
cut it. "Validate the client total matches the computed total" is a bug
waiting to happen — what's the error code, do we round, what if quantity
ever becomes a float upstream? The right answer is: never accept a client
total, period. Compute and store, full stop.

Adjacent decision: one of the auto-generated structs initially had
`quantity: f64`. All money paths in this codebase are `i64` cents and
quantities are `i32`. Overflow is checked on multiplication and sum.
There is no `f32` / `f64` anywhere in the payments code path — verifiable
with `grep -rE '\b(f32|f64)\b' crates/api/src crates/mock-psp/src migrations`.

## 3. One thing the AI got wrong (and how I caught it)

Claude initially wrote the webhook dispatcher's "claim" step as a plain
`SELECT … WHERE status='pending' AND next_attempt_at <= now()` followed by
an `UPDATE`. Under concurrent workers this races: two workers can claim the
same row, both POST to the receiver, and we double-deliver.

I changed it to a conditional `UPDATE`: bump `attempt_count`, advance
`next_attempt_at` by 60 s in the same statement, with `attempt_count = $2`
(the value we read) in the WHERE clause — only one worker's UPDATE takes
effect. If `rows_affected == 0`, we skip; another worker claimed it.

For the MVP there's only one dispatcher task, so the race isn't currently
observable. But the dispatcher is the first thing you'd scale horizontally,
and the original version would have looked fine in testing and shipped a
real bug. Catching it required asking "what happens if this worker runs
twice in parallel?" — which the AI did not volunteer.

How I verified the fixed version: walked through it as the only writer to
the row (UPDATE returns 1 → we proceed); as a loser in a race (some other
worker bumped `attempt_count` already → our UPDATE matches 0 rows → we
skip); under PSP failure with retries (each retry observes the bumped
`attempt_count` from the previous, picks the right backoff bucket). The
integration test wouldn't catch this — it would need two dispatchers
running, which we don't have yet.
