-- 0018 — Basket platform (phase 1)
--
-- A "Basket" is the existing `systems` row (UI name only; the table keeps its
-- name so every existing FK and query keeps working). This migration adds the
-- per-basket settings that used to be hard-coded, plus resource-level access.
--
--   basket_categories          category tree per basket (referenced by CODE)
--   basket_fields              field definitions per basket (values live in files.metadata JSON)
--   basket_statuses            status set per basket (codes = values stored in files.status)
--   basket_status_transitions  allowed status moves + the access level needed
--   basket_policies            per basket/folder policy (encrypt, allowed extensions, size, public links)
--   acl_entries                View/Edit/Manage grants on basket | folder | file
--
-- Access model (see acl.rs / fh_level below):
--   * a basket with NO basket-level grants keeps the old role behaviour (admin 3, editor 2, viewer 1)
--   * a basket WITH basket-level grants is strict at basket level; folder and file grants
--     always add on top; a folder with inherit=false cuts everything above it
--   * the file's creator always has at least Edit on their own file (not Manage, so a
--     creator cannot approve their own file when approval needs Manage)
--   * the owner of a personal basket (My Drive) has Manage on everything in it

CREATE TABLE IF NOT EXISTS basket_categories (
    id          TEXT PRIMARY KEY,
    system_id   TEXT NOT NULL REFERENCES systems(id) ON DELETE CASCADE,
    parent_code TEXT,
    code        TEXT NOT NULL,
    name        TEXT NOT NULL,
    sort        INT  NOT NULL DEFAULT 0,
    UNIQUE (system_id, code)
);

CREATE TABLE IF NOT EXISTS basket_fields (
    system_id    TEXT NOT NULL REFERENCES systems(id) ON DELETE CASCADE,
    key          TEXT NOT NULL,
    label        TEXT NOT NULL,
    field_type   TEXT NOT NULL DEFAULT 'text',   -- text | number | date | choice | user | org
    required     BOOLEAN NOT NULL DEFAULT false,
    choices      JSONB   NOT NULL DEFAULT '[]',
    searchable   BOOLEAN NOT NULL DEFAULT true,
    show_in_list BOOLEAN NOT NULL DEFAULT false,
    sort         INT     NOT NULL DEFAULT 0,
    PRIMARY KEY (system_id, key),
    CHECK (field_type IN ('text','number','date','choice','user','org')),
    CHECK (key ~ '^[a-z][a-z0-9_]{0,39}$')
);

CREATE TABLE IF NOT EXISTS basket_statuses (
    system_id     TEXT NOT NULL REFERENCES systems(id) ON DELETE CASCADE,
    code          TEXT NOT NULL,
    label         TEXT NOT NULL,
    color         TEXT NOT NULL DEFAULT 'gray',  -- gray | blue | green | orange | red (Carbon tag pairs)
    is_initial    BOOLEAN NOT NULL DEFAULT false,
    locks_content BOOLEAN NOT NULL DEFAULT false,
    sort          INT NOT NULL DEFAULT 0,
    PRIMARY KEY (system_id, code),
    CHECK (color IN ('gray','blue','green','orange','red'))
);

CREATE TABLE IF NOT EXISTS basket_status_transitions (
    system_id      TEXT NOT NULL REFERENCES systems(id) ON DELETE CASCADE,
    from_code      TEXT NOT NULL,
    to_code        TEXT NOT NULL,
    required_level SMALLINT NOT NULL DEFAULT 2,   -- 1 view, 2 edit, 3 manage
    PRIMARY KEY (system_id, from_code, to_code)
);

CREATE TABLE IF NOT EXISTS basket_policies (
    scope_type     TEXT NOT NULL,                 -- basket | folder
    scope_id       TEXT NOT NULL,
    encrypt        BOOLEAN,                       -- NULL = inherit
    allowed_ext    TEXT[],                        -- NULL = inherit, '{}' = any
    max_file_bytes BIGINT,                        -- NULL = inherit
    public_links   BOOLEAN,                       -- NULL = inherit
    PRIMARY KEY (scope_type, scope_id),
    CHECK (scope_type IN ('basket','folder'))
);

CREATE TABLE IF NOT EXISTS acl_entries (
    id            UUID PRIMARY KEY,
    system_id     TEXT NOT NULL REFERENCES systems(id) ON DELETE CASCADE,
    resource_type TEXT NOT NULL,                  -- basket | folder | file | vbasket
    resource_id   TEXT NOT NULL,
    principal     TEXT NOT NULL,                  -- user:<id> | org:<id> | role:<admin|editor|viewer> | iamx:<ROLE> | everyone
    level         SMALLINT NOT NULL,              -- 1 view, 2 edit, 3 manage
    expires_at    TIMESTAMPTZ,
    granted_by    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (resource_type, resource_id, principal),
    CHECK (resource_type IN ('basket','folder','file','vbasket')),
    CHECK (level BETWEEN 1 AND 3)
);
CREATE INDEX IF NOT EXISTS idx_acl_resource ON acl_entries(resource_type, resource_id);
CREATE INDEX IF NOT EXISTS idx_acl_system   ON acl_entries(system_id);

ALTER TABLE folders ADD COLUMN IF NOT EXISTS inherit BOOLEAN NOT NULL DEFAULT true;
ALTER TABLE files   ADD COLUMN IF NOT EXISTS category_code TEXT;
CREATE INDEX IF NOT EXISTS idx_files_category ON files(system_id, category_code);

-- IAMX identity: the stable subject (`sub`) is the key, email is only a display value.
ALTER TABLE users ADD COLUMN IF NOT EXISTS iamx_subject TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS iamx_roles   TEXT[] NOT NULL DEFAULT '{}';
CREATE UNIQUE INDEX IF NOT EXISTS ux_users_iamx_subject ON users(iamx_subject) WHERE iamx_subject IS NOT NULL;

-- Default status set for every existing basket, using the values already stored
-- in files.status so nothing has to be rewritten.
INSERT INTO basket_statuses (system_id, code, label, color, is_initial, locks_content, sort)
SELECT s.id, v.code, v.label, v.color, v.is_initial, v.locks, v.sort
  FROM systems s
 CROSS JOIN (VALUES
   ('Draft',    'Draft',     'gray',   true,  false, 1),
   ('Review',   'In review', 'orange', false, true,  2),
   ('Approved', 'Approved',  'green',  false, true,  3),
   ('Archived', 'Archived',  'gray',   false, true,  4)
 ) AS v(code, label, color, is_initial, locks, sort)
ON CONFLICT DO NOTHING;

INSERT INTO basket_status_transitions (system_id, from_code, to_code, required_level)
SELECT s.id, v.f, v.t, v.lvl
  FROM systems s
 CROSS JOIN (VALUES
   ('Draft','Review',2), ('Review','Approved',3), ('Review','Draft',3),
   ('Approved','Archived',3), ('Archived','Draft',3), ('Draft','Archived',2)
 ) AS v(f, t, lvl)
ON CONFLICT DO NOTHING;

-- Any status value already in use that the default set doesn't know becomes a
-- status of its basket too, so existing files never show an unknown status.
INSERT INTO basket_statuses (system_id, code, label, color, sort)
SELECT DISTINCT f.system_id, f.status, f.status, 'gray', 9
  FROM files f
ON CONFLICT DO NOTHING;

-- Access level of principals `p` on a file/folder/basket.
--   sys  basket id · fld folder id (NULL = basket root) · fid file id (NULL = folder/basket)
--   role FileHub role of the caller · creator = files.created_by · uid = caller id
--
-- Basket level: grants on the basket when it has any; otherwise the role
-- (admin 3 · editor 2 · viewer 1) — so a folder or file grant never locks the
-- rest of the basket. Folder and file grants add on top. A folder with
-- inherit=false drops everything above it (basket level included).
CREATE OR REPLACE FUNCTION fh_level(p TEXT[], role TEXT, sys TEXT, fld TEXT, fid UUID, creator TEXT, uid TEXT)
RETURNS INT LANGUAGE plpgsql STABLE AS $$
DECLARE
  lvl INT := 0;
  floor_lvl INT := 0;
  cur TEXT := fld;
  inh BOOLEAN := true;
  parent TEXT;
  depth INT := 0;
BEGIN
  IF role = 'admin' THEN RETURN 3; END IF;
  IF EXISTS (SELECT 1 FROM systems WHERE id = sys AND system_type = 'personal' AND owner_user_id = uid) THEN RETURN 3; END IF;
  IF fid IS NOT NULL AND creator IS NOT NULL AND creator = uid THEN floor_lvl := 2; END IF;
  IF fid IS NOT NULL THEN
    SELECT COALESCE(MAX(level), 0) INTO lvl FROM acl_entries
     WHERE resource_type = 'file' AND resource_id = fid::text AND principal = ANY(p)
       AND (expires_at IS NULL OR expires_at > now());
  END IF;
  WHILE cur IS NOT NULL AND depth < 64 LOOP
    lvl := GREATEST(lvl, COALESCE((SELECT MAX(level) FROM acl_entries
       WHERE resource_type = 'folder' AND resource_id = cur AND principal = ANY(p)
         AND (expires_at IS NULL OR expires_at > now())), 0));
    SELECT f.parent_id, f.inherit INTO parent, inh FROM folders f WHERE f.id = cur;
    IF NOT inh THEN RETURN GREATEST(lvl, floor_lvl); END IF;
    cur := parent;
    depth := depth + 1;
  END LOOP;
  IF EXISTS (SELECT 1 FROM acl_entries WHERE resource_type = 'basket' AND resource_id = sys) THEN
    lvl := GREATEST(lvl, COALESCE((SELECT MAX(level) FROM acl_entries
       WHERE resource_type = 'basket' AND resource_id = sys AND principal = ANY(p)
         AND (expires_at IS NULL OR expires_at > now())), 0));
  ELSE
    lvl := GREATEST(lvl, CASE role WHEN 'editor' THEN 2 WHEN 'viewer' THEN 1 ELSE 0 END);
  END IF;
  RETURN GREATEST(lvl, floor_lvl);
END $$;
