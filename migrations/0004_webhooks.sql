CREATE TABLE webhook_endpoints (
    id              UUID PRIMARY KEY,
    business_id     UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    url             TEXT NOT NULL,
    signing_secret  BYTEA NOT NULL,
    active          BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX webhook_endpoints_business_idx ON webhook_endpoints (business_id) WHERE active;

-- Outbox events: written in the same transaction as the state change that
-- emitted them. Dispatcher reads from here, fans out to webhook_deliveries.
CREATE TABLE outbox_events (
    id              UUID PRIMARY KEY,
    business_id     UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    event_type      TEXT NOT NULL,
    payload         JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    dispatched_at   TIMESTAMPTZ NULL
);

CREATE INDEX outbox_events_pending_idx ON outbox_events (created_at)
    WHERE dispatched_at IS NULL;

CREATE TABLE webhook_deliveries (
    id                   UUID PRIMARY KEY,
    outbox_event_id      UUID NOT NULL REFERENCES outbox_events(id) ON DELETE CASCADE,
    webhook_endpoint_id  UUID NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    attempt_count        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_status_code     INTEGER NULL,
    last_error           TEXT NULL,
    status               TEXT NOT NULL CHECK (status IN ('pending','delivered','failed')),
    delivered_at         TIMESTAMPTZ NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX webhook_deliveries_ready_idx
    ON webhook_deliveries (next_attempt_at)
    WHERE status = 'pending';
