-- Share links (TOR 4.15.11).
CREATE TABLE share_links (
    id          UUID PRIMARY KEY,
    file_id     UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    token       TEXT NOT NULL UNIQUE,
    expires_at  TIMESTAMPTZ,
    created_by  TEXT NOT NULL,
    note        TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_share_token ON share_links(token);
CREATE INDEX idx_share_file  ON share_links(file_id);
