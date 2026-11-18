-- 0021 — S3-compatible endpoint (replaces MinIO for systems that write files)
--
-- Access keys belong to a FileHub user; every S3 call is checked with that
-- user's View/Edit access. The secret is stored sealed (AES-GCM, key from
-- S3_CRED_KEY / STORAGE_ENC_KEY) because SigV4 needs the server to know it.
CREATE TABLE IF NOT EXISTS s3_credentials (
    access_key_id TEXT PRIMARY KEY,
    secret_sealed TEXT NOT NULL,
    label         TEXT NOT NULL,
    user_id       TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_by    TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at  TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS s3_multipart (
    upload_id  TEXT PRIMARY KEY,
    system_id  TEXT NOT NULL REFERENCES systems(id) ON DELETE CASCADE,
    object_key TEXT NOT NULL,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    headers    JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS s3_parts (
    upload_id   TEXT NOT NULL REFERENCES s3_multipart(upload_id) ON DELETE CASCADE,
    part_number INT  NOT NULL,
    path        TEXT NOT NULL,
    size        BIGINT NOT NULL,
    etag        TEXT NOT NULL,
    PRIMARY KEY (upload_id, part_number)
);
