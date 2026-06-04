# Video Script — Dodo Payments Take-Home

Target length: **6–8 minutes** (under the 10 min cap; aim for the middle).
Don't edit. Stumbles are fine. The graders are explicit: "We are not looking for polish. We are looking for fluency."

Before you hit record:

- Run `docker compose down -v` so the demo starts on a clean DB.
- Open these tabs/windows ahead of time so you don't fumble while live:
  1. Terminal A (where you'll type curls).
  2. Terminal B (`docker compose logs -f api` — viewer for state transitions).
  3. Editor with `DESIGN.md` open at §2 (state machine).
  4. Editor with `crates/api/src/routes/payments.rs` open at the top doc block.
  5. Editor with `crates/api/src/webhooks/reconciler.rs` open.
- Have a webhook receiver ready: `webhook.site` is easiest (gives you a public URL with no auth).
- Copy the host IP for the in-container webhook: `docker inspect --format='{{range .NetworkSettings.Networks}}{{.Gateway}}{{end}}' dodo-api-1` (only if you go with the local python receiver instead of webhook.site).

---

## Part 1 — Architecture overview (~90 s)

**What to show:** the file tree on the left, `DESIGN.md` section 1 on the right.

**What to say (rough script — paraphrase):**

> "Three services in `docker-compose.yml`: a Postgres, a mock PSP, and the API.
> The API is a single Actix-web binary. There's a second binary, `mock-psp`,
> that fakes the payment processor.
>
> The data model — I'll point at the ER sketch in DESIGN.md — has eight
> tables, all owned by `businesses` through `business_id`. The interesting
> pieces are: `payment_attempts` with a unique `(business_id, idempotency_key)`
> constraint — that's the idempotency primitive — and the `outbox_events` +
> `webhook_deliveries` pair, which is the outbox pattern for webhook fan-out.
>
> Request flow for a payment: client POSTs `/v1/invoices/{id}/pay` with an
> `Idempotency-Key` header. Auth middleware resolves the bearer token via a
> SHA-256 lookup against `api_keys.key_hash`. The handler runs two short
> transactions with the PSP HTTP call in between — we never hold a DB lock
> over network I/O. State changes write to `outbox_events` in the same
> transaction; a background tokio task picks those up, fans out one row per
> registered endpoint into `webhook_deliveries`, and POSTs with an
> HMAC-SHA256 signature. Delivery is decoupled — the API never blocks
> waiting for a webhook receiver.
>
> One more piece: a reconciler task that scans `payment_attempts` with
> `status='pending'` older than 10 seconds and looks them up on the PSP.
> That's how we handle timeouts and crashes — I'll come back to this."

**What to point at while speaking:**
- The three services in `docker-compose.yml`.
- The 4 migration files in `migrations/`.
- DESIGN.md section 1 — the table list.
- `lib.rs` showing the `/v1` scope and the workers being spawned in `main.rs`.

---

## Part 2 — Live demo (~2.5 min)

**What to show:** Terminal A for curls, Terminal B with `docker compose logs -f api` visible.

**What to say + do (step-by-step):**

### Step 2.1 — Start the stack

```sh
docker compose down -v
docker compose up --build
```

While it builds, say:

> "Clean machine — I just wiped the volume. This will run migrations on
> startup, bring up Postgres, the mock PSP, and the API. No manual setup
> steps."

Once you see all three healthy, switch to a new terminal tab so the logs stay visible in the original.

### Step 2.2 — Bootstrap a business + create a customer

```sh
BIZ=$(curl -fsS -X POST http://localhost:7002/v1/businesses \
  -H "Content-Type: application/json" \
  -d '{"name":"Acme Corp"}')
echo "$BIZ" | jq
KEY=$(echo "$BIZ" | jq -r .api_key)
```

Say:

> "Bootstrap returns the API key in plaintext, exactly once. The DB only
> stores the SHA-256 hash plus a short prefix for display."

```sh
CUST=$(curl -fsS -X POST http://localhost:7002/v1/customers \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"Jane Doe","email":"jane@example.com"}')
echo "$CUST" | jq
CID=$(echo "$CUST" | jq -r .id)
```

### Step 2.3 — Register a webhook endpoint

Open webhook.site in a browser, copy the unique URL, register it:

```sh
curl -fsS -X POST http://localhost:7002/v1/webhook_endpoints \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"url":"https://webhook.site/<your-id>"}' | jq
```

Say:

> "I'm registering a webhook endpoint pointed at webhook.site so we can see
> the signed delivery live."

### Step 2.4 — Create an invoice

```sh
INV=$(curl -fsS -X POST "http://localhost:7002/v1/invoices?finalize=true" \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d "{\"customer_id\":\"$CID\",\"line_items\":[
    {\"description\":\"Pro plan\",\"quantity\":1,\"unit_amount_cents\":4900},
    {\"description\":\"Add-on\",\"quantity\":2,\"unit_amount_cents\":1200}
  ]}")
echo "$INV" | jq
IID=$(echo "$INV" | jq -r .id)
```

Point at `total_cents` in the response:

> "Total is 7300. That was computed server-side from the line items — we
> never accept a client total. Money is integer cents end to end."

Switch to webhook.site briefly — show the `invoice.created` event arrived with a `Dodo-Signature` header.

### Step 2.5 — Successful payment

```sh
curl -fsS -X POST "http://localhost:7002/v1/invoices/$IID/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: pay-001" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_success"}' | jq
```

> "Returned `status: succeeded`. Now if I replay the same idempotency key —"

```sh
curl -fsS -X POST "http://localhost:7002/v1/invoices/$IID/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: pay-001" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_success"}' | jq
```

> "— same response, same attempt ID, same `psp_ref`. No second PSP call.
> That's the idempotency primitive working."

Switch to webhook.site — show the `invoice.paid` event arrived with its signature.

### Step 2.6 — Failed payment (declined card)

Create a fresh invoice (the previous one is `paid`):

```sh
INV2=$(curl -fsS -X POST "http://localhost:7002/v1/invoices?finalize=true" \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d "{\"customer_id\":\"$CID\",\"line_items\":[{\"description\":\"x\",\"quantity\":1,\"unit_amount_cents\":1000}]}")
IID2=$(echo "$INV2" | jq -r .id)

curl -fsS -X POST "http://localhost:7002/v1/invoices/$IID2/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: decline-1" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_card_declined"}' | jq
```

Then:

```sh
curl -fsS http://localhost:7002/v1/invoices/$IID2 \
  -H "Authorization: Bearer $KEY" | jq .state
```

Say:

> "Failed with `card_declined`. Invoice flipped back to `open` — the customer
> can retry with a different card. And on webhook.site you'll see an
> `invoice.payment_failed` event."

Show webhook.site one more time.

---

## Part 3 — State machine walkthrough (~75 s, UNSCRIPTED)

**What to show:** open `crates/api/src/domain/invoice_state.rs` in the editor, also `DESIGN.md` section 2.

**Talking points — say these in your own words, don't read:**

- "States: `draft`, `open`, `processing`, `paid`, `void`, `uncollectible`.
- `draft` is the create-but-not-yet-finalised state — in Stripe terms, you're
  still building line items. We have a `?finalize=true` shortcut on create
  for the demo.
- `open` means it's been finalised and can be paid.
- `processing` is the load-bearing one — that's the in-flight payment guard.
  Without it, two concurrent `/pay` requests with different idempotency keys
  could both pass the state check. With it, only one wins the
  `open → processing` transition.
- `paid`, `void`, `uncollectible` are terminal. No outgoing transitions —
  the unit tests at the bottom of this file assert that explicitly.
- One deliberation point: I considered creating invoices directly in `open`
  to skip the finalize dance. I kept the draft state because it matches the
  Stripe / canonical model and demonstrates a real state machine rather than
  a flag.
- The other deliberation: `processing → open` on PSP failure. I could have
  put declined payments into a separate `failed_payment` state, but reusing
  `open` keeps the retry path simple — the customer just tries again.
- All transitions go through one function: `try_transition(state, event)`.
  Invalid transitions return `Err` here, get mapped to HTTP 422 in the
  handler. Single source of truth."

Briefly scroll to the `try_transition` match arms in the file, and to the
unit tests below it.

---

## Part 4 — Failure-mode walkthrough (~90 s, UNSCRIPTED)

**Pick failure mode (b): `tok_timeout`.** It's the most interesting one to
demo live and the easiest to walk through code-wise.

**What to show, in order:**

1. `DESIGN.md` section 3, the (b) paragraph.
2. `crates/api/src/routes/payments.rs` — scroll to the match arm handling `PspError::Timeout`.
3. `crates/api/src/webhooks/reconciler.rs` — the whole file.
4. Terminal: run `tok_timeout` live.

**Talking points (your own words):**

- "The PSP's `tok_timeout` sleeps 30 seconds. Our endpoint must not block
  for that long. The reqwest client has a 5-second total timeout — when it
  fires we get `PspError::Timeout`.
- Look at `payments.rs` — this match arm. We *don't* roll back the invoice
  state. The invoice stays `processing`. The payment_attempt stays
  `pending`. We return 202 to the caller.
- The recovery is in `reconciler.rs`. Every 2 seconds it queries
  `payment_attempts WHERE status='pending' AND created_at < now() - 10s`.
  For each one, it calls `GET /psp/charges/{attempt_id}` on the PSP.
- Critical detail: when we sent the original POST, we used our `attempt_id`
  as the PSP's idempotency key. So the PSP knows that key. When the PSP
  eventually finishes the 30-second sleep and stores its outcome, the
  reconciler's lookup picks it up and runs Tx B.
- Same mechanism handles failure mode (c): if we crash between PSP success
  and our DB commit, the same lookup recovers the outcome on next restart.
  Customer is charged exactly once — we never re-POST to `/psp/charges` for
  the same attempt."

**Live demo:**

```sh
INV3=$(curl -fsS -X POST "http://localhost:7002/v1/invoices?finalize=true" \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d "{\"customer_id\":\"$CID\",\"line_items\":[{\"description\":\"x\",\"quantity\":1,\"unit_amount_cents\":2500}]}")
IID3=$(echo "$INV3" | jq -r .id)

time curl -sS -X POST "http://localhost:7002/v1/invoices/$IID3/pay" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: timeout-1" \
  -H "Content-Type: application/json" \
  -d '{"card_token":"tok_timeout"}' | jq
```

Point at the `time` output: ~5 s, not 30 s. Response is `pending`. Then:

```sh
curl -fsS http://localhost:7002/v1/invoices/$IID3 \
  -H "Authorization: Bearer $KEY" | jq .state
```

Shows `processing`. Then say: "Now we wait for the reconciler — the PSP's
actually still sleeping. Watch the logs."

In Terminal B (the logs viewer) point out the reconciler ticks. After ~25–30
seconds total, run the GET again:

```sh
curl -fsS http://localhost:7002/v1/invoices/$IID3 \
  -H "Authorization: Bearer $KEY" | jq .state
```

State is now `paid`. Done.

---

## Closing (~15 s, optional)

> "Everything not in the must-have list is in `DESIGN.md` section 6 —
> per-key scoping, refunds, rate limiting, observability. The production gap
> in section 7 is observability, rate limiting, audit log. That's the
> tour — happy to dig into any of it."

Stop recording.

---

## Last-minute checklist before you record

- [ ] `docker compose down -v` ran (clean state for the demo).
- [ ] webhook.site URL ready and pasted into your notes.
- [ ] Editor tabs pre-opened on the right files.
- [ ] `jq` installed (`apt install jq` or brew equivalent).
- [ ] Mic check — listen back to 5 seconds before doing the full run.
- [ ] Phone on silent.

If you fluff a curl or misspeak the state name, **keep going**. The graders
asked for "the working session, not a marketing reel."
