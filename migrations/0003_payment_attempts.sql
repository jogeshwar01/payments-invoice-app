-- Payment attempts: one row per call to POST /invoices/{id}/pay.
--
-- The UNIQUE (business_id, idempotency_key) constraint is the idempotency
-- primitive: a duplicate insert raises 23505, which the handler catches and
-- converts into a replay of the cached response.
--
-- request_hash protects against the key-reused-with-different-body case.
CREATE TABLE payment_attempts (
    id              UUID PRIMARY KEY,
    invoice_id      UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    business_id     UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    idempotency_key TEXT NOT NULL,
    request_hash    BYTEA NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('pending','succeeded','failed')),
    psp_ref         TEXT NULL,
    failure_code    TEXT NULL,
    response_json   JSONB NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at    TIMESTAMPTZ NULL
);

CREATE UNIQUE INDEX payment_attempts_idem_idx
    ON payment_attempts (business_id, idempotency_key);

CREATE INDEX payment_attempts_invoice_idx
    ON payment_attempts (invoice_id, created_at DESC);

-- Reconciler scans this index to find stale pending attempts.
CREATE INDEX payment_attempts_pending_idx
    ON payment_attempts (created_at)
    WHERE status = 'pending';
