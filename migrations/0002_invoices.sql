CREATE TABLE customers (
    id          UUID PRIMARY KEY,
    business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    email       TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX customers_business_email_idx ON customers (business_id, email);

-- Invoice state machine: draft -> open -> processing -> paid
-- Terminals: paid, void, uncollectible.
-- The 'processing' state is the in-flight payment guard.
CREATE TABLE invoices (
    id            UUID PRIMARY KEY,
    business_id   UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    customer_id   UUID NOT NULL REFERENCES customers(id),
    state         TEXT NOT NULL CHECK (state IN ('draft','open','processing','paid','void','uncollectible')),
    total_cents   BIGINT NOT NULL CHECK (total_cents >= 0),
    currency      TEXT NOT NULL DEFAULT 'USD',
    due_date      DATE NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX invoices_business_state_idx ON invoices (business_id, state, created_at DESC);
CREATE INDEX invoices_customer_idx ON invoices (customer_id);

CREATE TABLE invoice_line_items (
    id                  UUID PRIMARY KEY,
    invoice_id          UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    description         TEXT NOT NULL,
    quantity            INTEGER NOT NULL CHECK (quantity > 0),
    unit_amount_cents   BIGINT NOT NULL CHECK (unit_amount_cents >= 0),
    position            INTEGER NOT NULL
);

CREATE INDEX invoice_line_items_invoice_idx ON invoice_line_items (invoice_id, position);
