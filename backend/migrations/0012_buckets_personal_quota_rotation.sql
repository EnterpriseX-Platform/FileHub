-- Phase H/I/J/K — buckets, personal drives, per-scope quotas, rotation.
--
-- Adds the columns and tables the new endpoints need.  Idempotent against
-- a fresh DB (CREATE … IF NOT EXISTS / ADD COLUMN IF NOT EXISTS) so it can be
-- replayed on existing dev databases without manual reset.

-- -----------------------------------------------------------------------------
-- H/I/J — systems table extensions
-- -----------------------------------------------------------------------------
ALTER TABLE systems
    ADD COLUMN IF NOT EXISTS system_type   TEXT        NOT NULL DEFAULT 'shared',
    ADD COLUMN IF NOT EXISTS owner_user_id TEXT        REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS quota_bytes   BIGINT      NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS deleted_at    TIMESTAMPTZ;

-- 'shared' (default) — visible to everyone; existing seed systems are shared.
-- 'personal' — auto-created per-user "My Drive"; visible only to owner_user_id.
COMMENT ON COLUMN systems.system_type   IS 'shared | personal';
COMMENT ON COLUMN systems.quota_bytes   IS '0 = inherit (no system-level cap)';
COMMENT ON COLUMN systems.deleted_at    IS 'soft-delete tombstone';

CREATE INDEX IF NOT EXISTS idx_systems_owner    ON systems(owner_user_id) WHERE owner_user_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_systems_alive    ON systems(deleted_at) WHERE deleted_at IS NULL;

-- -----------------------------------------------------------------------------
-- J — per-scope quota_bytes for orgs + users (0 means "no cap at this scope")
-- -----------------------------------------------------------------------------
ALTER TABLE orgs
    ADD COLUMN IF NOT EXISTS quota_bytes BIGINT NOT NULL DEFAULT 0;
COMMENT ON COLUMN orgs.quota_bytes IS '0 = inherit (no org-level cap)';

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS quota_bytes BIGINT NOT NULL DEFAULT 0;
COMMENT ON COLUMN users.quota_bytes IS '0 = inherit (no per-user cap)';

-- -----------------------------------------------------------------------------
-- I — auto-create a personal drive for every existing user.  Bucket name is
-- prefixed with `personal-` so it can't collide with shared system buckets.
-- -----------------------------------------------------------------------------
INSERT INTO systems (id, name, tone, bucket, status, description, system_type, owner_user_id)
SELECT
    'sys_personal_' || u.id,
    'My Drive (' || u.display_name || ')',
    u.avatar_tone,
    'personal-' || u.id,
    'live',
    'Personal drive for ' || u.email,
    'personal',
    u.id
FROM users u
WHERE NOT EXISTS (
    SELECT 1 FROM systems s WHERE s.owner_user_id = u.id AND s.system_type = 'personal'
);

-- -----------------------------------------------------------------------------
-- K — rotation policies.  Scoped by `scope_type` + `scope_id`:
--   * workspace: scope_id IS NULL — applies to every file unless a more
--     specific policy overrides it.
--   * system   : scope_id = systems.id
--   * org      : scope_id = orgs.id
--   * user     : scope_id = users.id
-- The worker picks the most-specific policy that matches each file.
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS rotation_policies (
    id                     TEXT        PRIMARY KEY,
    scope_type             TEXT        NOT NULL,
    scope_id               TEXT,                        -- NULL for workspace
    keep_last_n_versions   INTEGER     NOT NULL DEFAULT 0,
    archive_after_days     INTEGER     NOT NULL DEFAULT 0,
    delete_after_days      INTEGER     NOT NULL DEFAULT 0,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT rotation_scope_chk
        CHECK (scope_type IN ('workspace','system','org','user')),
    CONSTRAINT rotation_workspace_scope_chk
        CHECK ((scope_type = 'workspace' AND scope_id IS NULL) OR
               (scope_type <> 'workspace' AND scope_id IS NOT NULL))
);
COMMENT ON COLUMN rotation_policies.keep_last_n_versions IS '0 = keep all versions';
COMMENT ON COLUMN rotation_policies.archive_after_days   IS '0 = never archive (soft-delete)';
COMMENT ON COLUMN rotation_policies.delete_after_days    IS '0 = never hard-delete trashed files';

CREATE UNIQUE INDEX IF NOT EXISTS uq_rotation_workspace ON rotation_policies (scope_type) WHERE scope_type = 'workspace';
CREATE UNIQUE INDEX IF NOT EXISTS uq_rotation_scoped    ON rotation_policies (scope_type, scope_id) WHERE scope_id IS NOT NULL;

-- Sensible workspace defaults: keep 5 versions, never auto-archive, hard-delete
-- trashed items after 30 days.  Admins can change these via the settings UI.
INSERT INTO rotation_policies (id, scope_type, scope_id, keep_last_n_versions, archive_after_days, delete_after_days)
VALUES ('rot_workspace_default', 'workspace', NULL, 5, 0, 30)
ON CONFLICT DO NOTHING;

-- -----------------------------------------------------------------------------
-- K — rotation run log so admins can see what fired and when.
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS rotation_runs (
    id                 UUID        PRIMARY KEY,
    started_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at        TIMESTAMPTZ,
    versions_pruned    INTEGER     NOT NULL DEFAULT 0,
    files_archived     INTEGER     NOT NULL DEFAULT 0,
    files_hard_deleted INTEGER     NOT NULL DEFAULT 0,
    triggered_by       TEXT,                            -- user id, or 'cron'
    error              TEXT
);
CREATE INDEX IF NOT EXISTS idx_rotation_runs_started ON rotation_runs(started_at DESC);
