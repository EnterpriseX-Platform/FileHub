-- 0039 — UAT findings 17/11/2569
--
-- Public links: what the recipient may do (view only / view and download) and
-- whether they must be signed in to FileHub first.
ALTER TABLE share_links ADD COLUMN IF NOT EXISTS access        TEXT    NOT NULL DEFAULT 'download';
ALTER TABLE share_links ADD COLUMN IF NOT EXISTS require_login BOOLEAN NOT NULL DEFAULT false;

-- Versions: the note and uploader of the CURRENT version live on the file, so
-- when a newer version arrives the archived row keeps its own note (not the
-- note typed for the newer one).
ALTER TABLE files ADD COLUMN IF NOT EXISTS version_note TEXT;
ALTER TABLE files ADD COLUMN IF NOT EXISTS version_by   TEXT;

-- Folder policy: keep earlier versions when a new one is uploaded (NULL = inherit, default on).
ALTER TABLE basket_policies ADD COLUMN IF NOT EXISTS keep_versions BOOLEAN;

-- Image renditions (300 / 600 / 1024 px), made once and served from here after.
CREATE TABLE IF NOT EXISTS image_renditions (
    file_id    UUID        NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    width      INT         NOT NULL,
    etag       TEXT,
    mime       TEXT        NOT NULL,
    px_w       INT         NOT NULL,
    px_h       INT         NOT NULL,
    data       BYTEA       NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (file_id, width)
);

-- Folder search by name.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_trgm') THEN
    EXECUTE 'CREATE INDEX IF NOT EXISTS idx_folders_name_trgm ON folders USING GIN (name gin_trgm_ops)';
  END IF;
END $$;
