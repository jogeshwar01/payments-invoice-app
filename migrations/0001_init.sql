-- Businesses are the auth boundary. Every other table FK's back here through business_id.
CREATE TABLE businesses (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- API keys are stored as SHA-256 hashes. The prefix is kept plaintext for
-- dashboard display ("dodo_sk_live_abc1..."). Plaintext is shown ONCE on
-- creation and never persisted.
CREATE TABLE api_keys (
    id          UUID PRIMARY KEY,
    business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    key_hash    BYTEA NOT NULL,
    key_prefix  TEXT NOT NULL,
    name        TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at  TIMESTAMPTZ NULL
);

CREATE UNIQUE INDEX api_keys_key_hash_idx ON api_keys (key_hash);
CREATE INDEX api_keys_business_id_idx ON api_keys (business_id);
