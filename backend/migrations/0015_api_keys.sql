-- API keys for machine-to-machine auth (headless upload + integrations).
--
-- A key authenticates AS a user (service account): its role and system access
-- are exactly the linked user's, so the existing require_role /
-- ensure_system_access / effective_system_ids checks apply unchanged.
--
-- The plaintext key (`fhk_<random>`) is shown ONCE at creation; only a
-- SHA-256 hash is stored (keys are high-entropy, so a fast hash is fine — no
-- argon2 per request). Revocation is immediate (revoked_at) and a key may
-- carry an optional expiry.
--
-- ID strategy: api_keys is a transaction table → UUID PK (UUIDv7 from the app).
-- user_id / created_by reference the TEXT user PKs.
CREATE TABLE IF NOT EXISTS api_keys (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL,
    key_hash     TEXT NOT NULL UNIQUE,        -- hex(sha256(plaintext))
    key_prefix   TEXT NOT NULL,               -- e.g. "fhk_a1b2c3d4" for display
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_by   TEXT          REFERENCES users(id) ON DELETE SET NULL,
    last_used_at TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Hot path: look up a live key by hash on every Bearer-authenticated request.
CREATE INDEX IF NOT EXISTS api_keys_key_hash_live_idx ON api_keys (key_hash) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS api_keys_user_idx          ON api_keys (user_id);
