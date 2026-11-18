-- 0023 — Upload links ("file requests")
--
-- Someone with Edit on a basket or folder creates a link; anyone holding the
-- link (or, with require_login, anyone signed in) can drop files into that
-- place through a small form — without access to see what's already there.
-- Files are stored as uploaded by the link's creator, with the submitter's
-- name/email kept in the file's fields.
CREATE TABLE IF NOT EXISTS upload_links (
    token          TEXT PRIMARY KEY,
    system_id      TEXT NOT NULL REFERENCES systems(id) ON DELETE CASCADE,
    folder_id      TEXT REFERENCES folders(id) ON DELETE CASCADE,
    title          TEXT NOT NULL,
    message        TEXT,
    require_login  BOOLEAN NOT NULL DEFAULT false,
    expires_at     TIMESTAMPTZ NOT NULL,
    max_files      INT,
    upload_count   INT NOT NULL DEFAULT 0,
    created_by     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at     TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_upload_links_place ON upload_links(system_id, folder_id) WHERE revoked_at IS NULL;
