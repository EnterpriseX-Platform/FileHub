-- 0019 — Virtual Baskets + download list (phase 2)
--
-- A Virtual Basket stores no files. It is a saved rule set over files that live
-- in real baskets: Sources (which baskets) + Rules (match all/any) + Group by
-- (field order that turns into folders) + an optional Drop target (where an
-- upload made from the virtual basket really goes).
--
-- Rules are evaluated when read (vbasket.rs compiles them to SQL); nothing is
-- copied and no membership is stored, so a new or edited file shows up at once.

CREATE TABLE IF NOT EXISTS vbaskets (
    id                    TEXT PRIMARY KEY,
    name                  TEXT NOT NULL,
    description           TEXT,
    match_mode            TEXT NOT NULL DEFAULT 'all',      -- all | any
    rules                 JSONB NOT NULL DEFAULT '[]',
    group_by              TEXT[] NOT NULL DEFAULT '{}',
    all_sources           BOOLEAN NOT NULL DEFAULT false,   -- true = every basket the viewer can see
    drop_target_system_id TEXT REFERENCES systems(id) ON DELETE SET NULL,
    drop_target_folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL,
    columns               TEXT[] NOT NULL DEFAULT '{}',
    personal              BOOLEAN NOT NULL DEFAULT false,   -- true = only the creator sees it
    shared_with           TEXT[] NOT NULL DEFAULT '{}',     -- principals (acl.rs format) who may open it
    created_by            TEXT NOT NULL REFERENCES users(id),
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at            TIMESTAMPTZ,
    CHECK (match_mode IN ('all','any'))
);

CREATE TABLE IF NOT EXISTS vbasket_sources (
    vbasket_id TEXT NOT NULL REFERENCES vbaskets(id) ON DELETE CASCADE,
    system_id  TEXT NOT NULL REFERENCES systems(id) ON DELETE CASCADE,
    PRIMARY KEY (vbasket_id, system_id)
);

CREATE TABLE IF NOT EXISTS vbasket_excludes (
    vbasket_id TEXT NOT NULL REFERENCES vbaskets(id) ON DELETE CASCADE,
    file_id    UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    excluded_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (vbasket_id, file_id)
);

-- Download list: one per user, survives devices.
CREATE TABLE IF NOT EXISTS download_lists (
    user_id    TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    items      JSONB NOT NULL DEFAULT '[]',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_files_status ON files(system_id, status) WHERE deleted_at IS NULL;

-- Extension of a file name, lower-case ('' when none).
CREATE OR REPLACE FUNCTION fh_ext(n TEXT) RETURNS TEXT LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE WHEN n ~ '\.[^./]+$' THEN lower(substring(n from '\.([^./]+)$')) ELSE '' END
$$;

-- Format group used by rules and Group by.
CREATE OR REPLACE FUNCTION fh_format(n TEXT) RETURNS TEXT LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE
    WHEN fh_ext(n) = 'pdf' THEN 'PDF'
    WHEN fh_ext(n) IN ('doc','docx','odt','rtf') THEN 'Word'
    WHEN fh_ext(n) IN ('xls','xlsx','ods','csv') THEN 'Excel'
    WHEN fh_ext(n) IN ('ppt','pptx','odp') THEN 'PowerPoint'
    WHEN fh_ext(n) IN ('png','jpg','jpeg','gif','webp','bmp','tif','tiff','svg','heic') THEN 'Image'
    WHEN fh_ext(n) IN ('wav','mp3','m4a','aac','ogg','flac') THEN 'Audio'
    WHEN fh_ext(n) IN ('mp4','mov','webm','avi','mkv') THEN 'Video'
    WHEN fh_ext(n) IN ('txt','md','html','htm','xml','json') THEN 'Text'
    WHEN fh_ext(n) IN ('zip','rar','7z','gz','tar') THEN 'Archive'
    ELSE 'Other' END
$$;

-- Field values of a file as JSON (files.metadata is TEXT for history reasons).
CREATE OR REPLACE FUNCTION fh_meta(m TEXT) RETURNS JSONB LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE WHEN m ~ '^\s*\{' THEN m::jsonb ELSE '{}'::jsonb END
$$;

-- Rule filters on field values read files.metadata through fh_meta().
CREATE INDEX IF NOT EXISTS idx_files_meta_gin ON files USING GIN (fh_meta(metadata));
CREATE INDEX IF NOT EXISTS idx_files_format ON files (fh_format(name)) WHERE deleted_at IS NULL;
