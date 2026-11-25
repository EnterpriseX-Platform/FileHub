-- TUS resumable-upload sessions and Office → PDF previews.

CREATE TABLE tus_uploads (
    id             UUID PRIMARY KEY,
    user_id        TEXT REFERENCES users(id) ON DELETE SET NULL,
    upload_length  BIGINT NOT NULL,            -- final byte size declared by client
    upload_offset  BIGINT NOT NULL DEFAULT 0,  -- bytes received so far
    filename       TEXT,
    content_type   TEXT,
    system_id      TEXT REFERENCES systems(id),
    org_id         TEXT REFERENCES orgs(id),
    folder_id      TEXT REFERENCES folders(id),
    project        TEXT,
    status_meta    TEXT,                       -- desired file row "status" once complete
    owner_meta     TEXT,
    tags_meta      TEXT,
    temp_key       TEXT NOT NULL,              -- path under tus temp dir for partial bytes
    file_id        UUID REFERENCES files(id) ON DELETE SET NULL,  -- non-null once finalised
    completed_at   TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '24 hours')
);
CREATE INDEX idx_tus_user    ON tus_uploads(user_id);
CREATE INDEX idx_tus_expires ON tus_uploads(expires_at) WHERE completed_at IS NULL;

-- Rendered preview bytes (PDF) for Word/Excel/PowerPoint/ODF source files.
CREATE TABLE file_previews (
    file_id     UUID PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    data        BYTEA NOT NULL,
    mime        TEXT NOT NULL DEFAULT 'application/pdf',
    page_count  INT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
