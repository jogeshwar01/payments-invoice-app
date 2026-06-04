# Webhooks, Auth, And Security

## Webhook design

### Events

Minimum assignment events:

- `invoice.created`
- `invoice.paid`
- `invoice.payment_failed`

Current implementation writes events to `outbox_events`.

### Outbox pattern

Why:

- The event is persisted in the same transaction as the invoice/payment state.
- API responses do not wait for external webhook receivers.
- Failed delivery can be retried without re-running business logic.

Flow:

```text
state change transaction
  -> insert outbox_events
  -> commit

dispatcher
  -> fan out event to webhook_deliveries per active endpoint
  -> POST signed body
  -> mark delivered or schedule retry
```

### Signing

Header:

```text
Dodo-Signature: t=<unix_ts>,v1=<hex_hmac>
```

Signed material:

```text
"{timestamp}.{raw_body}"
```

Algorithm:

- HMAC-SHA256 with per-endpoint secret.
- The API returns the secret as `whsec_<base64url>`. A receiver should strip `whsec_`, base64url-decode it to raw bytes, and use those bytes as the HMAC key.

Replay protection:

- Receiver rejects timestamp older/newer than 5 minutes.
- Timestamp is included in the HMAC, so attacker cannot alter it.

Constant-time comparison:

- Receivers should compare computed and supplied HMAC with constant-time equality.

Why HMAC:

- Common webhook pattern.
- Easy for receivers to implement.
- No public key distribution problem.

If asked why not Ed25519:

> "Asymmetric signing is useful when receivers should not hold a shared secret or when key distribution is mature. For this assignment, HMAC is simpler, standard, and enough."

### Retry policy

Schedule:

- 30 seconds
- 2 minutes
- 10 minutes
- 1 hour
- 6 hours
- 24 hours

After max attempts:

- Delivery is marked `failed`.
- Event remains in outbox/delivery history.

Production addition:

- `GET /v1/events?since=...` to let businesses reconcile missed webhooks.
- Dashboard/manual replay.
- Alerting on failing endpoints.
- Explicit endpoint snapshot semantics: decide whether events go to endpoints active at event time or endpoints active at dispatch time.

### Delivery claim

The dispatcher uses a conditional update:

```text
UPDATE webhook_deliveries
SET attempt_count = attempt_count + 1,
    next_attempt_at = now() + interval '60 seconds'
WHERE id = $1
  AND status = 'pending'
  AND attempt_count = $2
```

Purpose:

- Avoids two worker instances delivering the same row at the same time.
- If rows affected is zero, another worker claimed it.

MVP note:

- There is one dispatcher task now.
- This pattern makes horizontal worker scale safer later.

## API key model

### Generation

Current code:

- 32 random bytes.
- Base64url encoded.
- Prefix `dodo_sk_live_`.

Docs mention `OsRng`; code uses `rand::thread_rng()`.

How to explain:

> "`thread_rng()` in rand is a CSPRNG seeded from the OS, so the security property is still high-entropy random keys. I should make docs and code match, either by documenting ThreadRng or switching to OsRng for clarity."

### Storage

Database stores:

- `key_hash`: SHA-256 hash of full plaintext key.
- `key_prefix`: short display prefix.
- `revoked_at`: nullable timestamp.

Plaintext:

- Returned once.
- Not stored.

### Why SHA-256 instead of bcrypt

Good answer:

> "Bcrypt is for user passwords, which are low entropy and guessable. API keys are 32 random bytes. A slow KDF would add latency to every API request without materially improving brute-force resistance against a 256-bit random key."

### Transmission

Header:

```text
Authorization: Bearer dodo_sk_live_...
```

Production:

- Require TLS.
- Avoid logging bearer tokens.
- Consider key prefixes in logs, never full keys.

### Rotation

Current model supports multiple active keys per business.

Desired process:

1. Create new key.
2. Update client configuration.
3. Revoke old key.

MVP gap:

- `revoked_at` exists but no authenticated revoke endpoint is implemented.
- Additional API-key creation route is unauthenticated; see known gaps.

### Blast radius

Current:

- Full business scope.

Production mitigations:

- Per-key scopes.
- Per-key rate limits.
- IP allowlists.
- Audit log with `api_key_id`.
- Secret scanning alerts.

## Security questions likely to come up

### Is URL validation enough for webhooks?

No.

Current:

- Only checks `http://` or `https://`.

Production:

- SSRF protections.
- Block localhost, link-local, private IP ranges unless explicitly allowed.
- Resolve DNS and validate final IP.
- Limit redirects.
- Egress proxy allowlist.
- Timeout and body size limits.

### What if webhook secret leaks?

Impact:

- Attacker can forge events for that endpoint.

Mitigation:

- Rotate endpoint secret.
- Include timestamp and reject replays.
- Let receivers verify event IDs by querying API.

### What if API key leaks?

Impact:

- Attacker can act as the business.

Mitigation:

- Revoke key.
- Audit actions.
- Per-key scopes and rate limits.
- Alert on unusual payment volume.

### What should not be logged?

Avoid logging:

- Full API keys.
- Webhook secrets.
- Card tokens in production.
- Full request bodies containing secrets.

MVP note:

- Mock card tokens are not real PAN/card data, but in a real PSP integration they should be treated carefully.
