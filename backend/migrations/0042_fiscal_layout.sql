-- 0042 — fiscal-year folder layout for a basket
--
-- <year>/<parent organisation>/<organisation>/<main group>/<source>[/<stage>]
-- <year>/<support group>
-- See src/layout.rs. Nothing here applies until a layout_settings row exists.

-- Organisations can sit under another one (ministry → department).
ALTER TABLE orgs ADD COLUMN IF NOT EXISTS parent_id TEXT REFERENCES orgs(id);
CREATE INDEX IF NOT EXISTS idx_orgs_parent ON orgs(parent_id);

CREATE TABLE IF NOT EXISTS layout_settings (
    system_id      TEXT    PRIMARY KEY REFERENCES systems(id),
    enabled        BOOLEAN NOT NULL DEFAULT true,
    -- NULL = from the date: the fiscal year starts in fy_start_month, numbered +year_offset.
    current_year   INT,
    fy_start_month INT     NOT NULL DEFAULT 10,
    year_offset    INT     NOT NULL DEFAULT 0,
    main_name      TEXT    NOT NULL DEFAULT 'Main',
    support_name   TEXT    NOT NULL DEFAULT 'Support'
);

-- Systems that send files: main group (a folder per organisation) or support (one shared folder).
CREATE TABLE IF NOT EXISTS layout_sources (
    id            TEXT    PRIMARY KEY DEFAULT fh_cuid(),
    system_id     TEXT    NOT NULL REFERENCES systems(id),
    code          TEXT    NOT NULL,
    folder_name   TEXT    NOT NULL,
    grp           TEXT    NOT NULL CHECK (grp IN ('main', 'support')),
    keep_versions BOOLEAN NOT NULL DEFAULT false,
    path_prefixes TEXT[]  NOT NULL DEFAULT '{}',
    UNIQUE (system_id, code)
);

-- Stages per fiscal year; a stage folder is "<code> <name>".
CREATE TABLE IF NOT EXISTS layout_stages (
    id          TEXT    PRIMARY KEY DEFAULT fh_cuid(),
    system_id   TEXT    NOT NULL REFERENCES systems(id),
    fiscal_year INT     NOT NULL,
    source_code TEXT    NOT NULL,
    code        TEXT    NOT NULL,
    name        TEXT    NOT NULL,
    sort        INT     NOT NULL DEFAULT 0,
    active      BOOLEAN NOT NULL DEFAULT true,
    UNIQUE (system_id, fiscal_year, code)
);

-- Folders the layout made, by path, so each is made once and renamed in place.
CREATE TABLE IF NOT EXISTS layout_folders (
    system_id TEXT NOT NULL REFERENCES systems(id),
    path_key  TEXT NOT NULL,
    folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    PRIMARY KEY (system_id, path_key)
);
