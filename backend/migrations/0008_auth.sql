-- Users + sessions for cookie-based auth.
--
-- Users are a system table → TEXT/CUID2 PK with readable seed IDs:
--   usr_admin   admin@acme.go.th    / admin123    (admin)
--   usr_anong   anong@acme.go.th    / anong123    (editor)
--   usr_viewer  viewer@acme.go.th   / viewer123   (viewer)
-- Hashes are bootstrapped from Rust (see state.rs::bootstrap_seed_users).
CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    avatar_tone   TEXT NOT NULL DEFAULT 'slate',
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'viewer',
    status        TEXT NOT NULL DEFAULT 'active',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_users_email ON users(email);

-- Sessions are a transaction table → UUIDv7.
CREATE TABLE sessions (
    id           UUID PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token        TEXT NOT NULL UNIQUE,
    expires_at   TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_sessions_token ON sessions(token);
CREATE INDEX idx_sessions_user  ON sessions(user_id);

ALTER TABLE files       ADD COLUMN created_by    TEXT REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE folders     ADD COLUMN created_by    TEXT REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE views       ADD COLUMN created_by    TEXT REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE share_links ADD COLUMN created_by_id TEXT REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE activity    ADD COLUMN actor_id      TEXT REFERENCES users(id) ON DELETE SET NULL;
