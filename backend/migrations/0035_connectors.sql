-- Connectors: each outside system that calls FileHub, configured from the admin
-- console instead of environment variables (no deployment-specific names in code).
CREATE TABLE IF NOT EXISTS connectors (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL DEFAULT 'legacy_fileservice',
    enabled       BOOLEAN NOT NULL DEFAULT true,
    read_only     BOOLEAN NOT NULL DEFAULT false,
    token_hash    TEXT UNIQUE,                 -- SHA-256 of the caller's fixed token
    token_hint    TEXT,                        -- last 4 characters, to recognise it
    path_prefixes TEXT[] NOT NULL DEFAULT '{}', -- app path prefixes seen in Referer
    system_id     TEXT REFERENCES systems(id) ON DELETE SET NULL,
    org_id        TEXT,
    role          TEXT NOT NULL DEFAULT 'editor',
    calls         BIGINT NOT NULL DEFAULT 0,
    last_used_at  TIMESTAMPTZ,
    created_by    TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
