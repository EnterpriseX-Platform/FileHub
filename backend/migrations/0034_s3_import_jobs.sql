-- Importing a whole bucket from MinIO/S3: one row per run, progress visible
-- from any pod.
CREATE TABLE IF NOT EXISTS s3_import_jobs (
    id          UUID PRIMARY KEY,
    source      TEXT NOT NULL,          -- endpoint/bucket/prefix (no secrets)
    system_id   TEXT NOT NULL REFERENCES systems(id) ON DELETE CASCADE,
    started_by  TEXT REFERENCES users(id) ON DELETE SET NULL,
    status      TEXT NOT NULL DEFAULT 'running',   -- running | done | failed
    copied      INT NOT NULL DEFAULT 0,
    skipped     INT NOT NULL DEFAULT 0,
    failed      INT NOT NULL DEFAULT 0,
    bytes       BIGINT NOT NULL DEFAULT 0,
    last_error  TEXT,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);
