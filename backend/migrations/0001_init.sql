-- Hybrid key strategy:
--   - "System" tables (systems, orgs, folders, views, users) use TEXT primary
--     keys generated as CUID2 by the application.  Seed rows use readable
--     literals like `sys_hr`, `org_phattana` so tests + URLs stay debuggable.
--   - "Transaction" tables (files, activity, sessions, share_links, comments,
--     notifications, workflow*, file_versions, thumbnails) use UUIDv7 PKs
--     so insert volume + index locality scale well.
--   - FK columns track the type of the referenced PK.

CREATE TABLE systems (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    tone         TEXT NOT NULL,
    bucket       TEXT NOT NULL UNIQUE,
    status       TEXT NOT NULL DEFAULT 'live',
    description  TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE orgs (
    id           TEXT PRIMARY KEY,
    system_id    TEXT NOT NULL REFERENCES systems(id),
    name         TEXT NOT NULL,
    code         TEXT NOT NULL UNIQUE,
    tier         TEXT NOT NULL,
    owner        TEXT NOT NULL,
    tags         TEXT NOT NULL DEFAULT '[]',
    status       TEXT NOT NULL DEFAULT 'Active',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_orgs_system ON orgs(system_id);

CREATE TABLE files (
    id            UUID PRIMARY KEY,
    name          TEXT NOT NULL,
    file_type     TEXT NOT NULL,
    size_bytes    BIGINT NOT NULL DEFAULT 0,
    system_id     TEXT NOT NULL REFERENCES systems(id),
    org_id        TEXT REFERENCES orgs(id),
    bucket        TEXT NOT NULL,
    object_key    TEXT NOT NULL,
    project       TEXT,
    status        TEXT NOT NULL DEFAULT 'Draft',
    owner         TEXT NOT NULL,
    tags          TEXT NOT NULL DEFAULT '[]',
    version       BIGINT NOT NULL DEFAULT 1,
    metadata      TEXT NOT NULL DEFAULT '{}',
    etag          TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    modified_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_files_system ON files(system_id);
CREATE INDEX idx_files_org    ON files(org_id);
CREATE INDEX idx_files_status ON files(status);
CREATE INDEX idx_files_modified ON files(modified_at DESC);

CREATE TABLE activity (
    id          UUID PRIMARY KEY,
    actor       TEXT NOT NULL,
    actor_tone  TEXT NOT NULL DEFAULT 'slate',
    action      TEXT NOT NULL,
    target      TEXT,
    target_type TEXT,
    file_id     UUID REFERENCES files(id) ON DELETE SET NULL,
    system_id   TEXT REFERENCES systems(id),
    org_id      TEXT REFERENCES orgs(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_activity_created ON activity(created_at DESC);

CREATE TABLE views (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    layout      TEXT NOT NULL DEFAULT 'table',
    source      TEXT NOT NULL DEFAULT '{}',
    filters     TEXT NOT NULL DEFAULT '[]',
    group_by    TEXT,
    sort_by     TEXT,
    fields      TEXT NOT NULL DEFAULT '[]',
    pinned      BOOLEAN NOT NULL DEFAULT false,
    color       TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE permissions (
    id          UUID PRIMARY KEY,
    file_id     UUID REFERENCES files(id) ON DELETE CASCADE,
    org_id      TEXT REFERENCES orgs(id),
    principal   TEXT NOT NULL,
    principal_type TEXT NOT NULL DEFAULT 'user',
    role        TEXT NOT NULL DEFAULT 'view',
    external    BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_perm_file ON permissions(file_id);
