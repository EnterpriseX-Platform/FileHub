-- File version history (TOR 4.15.7 — versioning).
CREATE TABLE file_versions (
    id           UUID PRIMARY KEY,
    file_id      UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    version      BIGINT NOT NULL,
    object_key   TEXT NOT NULL,
    size_bytes   BIGINT NOT NULL DEFAULT 0,
    etag         TEXT,
    uploaded_by  TEXT NOT NULL,
    note         TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_file_versions_unique ON file_versions(file_id, version);
CREATE INDEX idx_file_versions_file ON file_versions(file_id, version DESC);
