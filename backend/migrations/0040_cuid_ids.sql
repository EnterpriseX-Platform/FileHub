-- Every key is a CUID2-style id: 24 characters, a lowercase letter then
-- lowercase letters and digits (e.g. "k8x2m9q4…"). No prefixes, no UUIDs.
-- Rewrites every existing id and every column that points at one; the old
-- ids are not kept.

-- Random id for rows created by SQL (the app makes its own with cuid2).
CREATE OR REPLACE FUNCTION fh_cuid() RETURNS text LANGUAGE plpgsql VOLATILE AS $$
DECLARE
  b bytea := uuid_send(gen_random_uuid()) || uuid_send(gen_random_uuid());
  d constant text := '0123456789abcdefghijklmnopqrstuvwxyz';
  r text := substr('abcdefghijklmnopqrstuvwxyz', 1 + get_byte(b, 0) % 26, 1);
BEGIN
  FOR i IN 1..23 LOOP
    r := r || substr(d, 1 + get_byte(b, i) % 36, 1);
  END LOOP;
  RETURN r;
END $$;

-- A connector's own account used to be found as "usr_" || connector id.
ALTER TABLE users ADD COLUMN IF NOT EXISTS connector_id TEXT;
UPDATE users u SET connector_id = c.id FROM connectors c WHERE u.id = 'usr_' || c.id;
ALTER TABLE users ADD CONSTRAINT users_connector_id_fkey FOREIGN KEY (connector_id) REFERENCES connectors(id) ON DELETE SET NULL;
CREATE UNIQUE INDEX IF NOT EXISTS users_connector_id_key ON users (connector_id) WHERE connector_id IS NOT NULL;

DO $$
DECLARE
  r record;
  t text;
BEGIN
  -- 1. Foreign keys off (kept to put back at the end).
  CREATE TEMP TABLE _fk ON COMMIT DROP AS
    SELECT c.conrelid::regclass::text AS tbl, c.conname, pg_get_constraintdef(c.oid) AS def,
           c.confrelid::regclass::text AS ref, a.attname::text AS col, af.attname::text AS refcol
      FROM pg_constraint c
      JOIN pg_attribute a  ON a.attrelid = c.conrelid  AND a.attnum = c.conkey[1]
      JOIN pg_attribute af ON af.attrelid = c.confrelid AND af.attnum = c.confkey[1]
     WHERE c.contype = 'f' AND c.connamespace = 'public'::regnamespace;
  FOR r IN SELECT * FROM _fk LOOP
    EXECUTE format('ALTER TABLE %s DROP CONSTRAINT %I', r.tbl, r.conname);
  END LOOP;

  -- 2. UUID columns become text.
  FOR r IN SELECT table_name, column_name, column_default FROM information_schema.columns
            WHERE table_schema = 'public' AND data_type = 'uuid' LOOP
    IF r.column_default IS NOT NULL THEN
      EXECUTE format('ALTER TABLE %I ALTER COLUMN %I DROP DEFAULT', r.table_name, r.column_name);
    END IF;
    EXECUTE format('ALTER TABLE %I ALTER COLUMN %I TYPE text USING %I::text', r.table_name, r.column_name, r.column_name);
  END LOOP;

  -- 3. New id for every row of every table keyed by "id".
  CREATE TEMP TABLE _map (tbl text, old text, new text, PRIMARY KEY (tbl, old)) ON COMMIT DROP;
  FOR t IN SELECT c.conrelid::regclass::text FROM pg_constraint c
            WHERE c.contype = 'p' AND c.connamespace = 'public'::regnamespace
              AND array_length(c.conkey, 1) = 1
              AND (SELECT attname FROM pg_attribute WHERE attrelid = c.conrelid AND attnum = c.conkey[1]) = 'id' LOOP
    EXECUTE format('INSERT INTO _map SELECT %L, id::text, fh_cuid() FROM %s', t, t);
    EXECUTE format('UPDATE %s x SET id = m.new FROM _map m WHERE m.tbl = %L AND m.old = x.id', t, t);
    EXECUTE format('ALTER TABLE %s ALTER COLUMN id SET DEFAULT fh_cuid()', t);
  END LOOP;

  -- 4. Every column pointing at a remapped id follows it.
  FOR r IN SELECT * FROM _fk WHERE refcol = 'id' LOOP
    EXECUTE format('UPDATE %s x SET %I = m.new FROM _map m WHERE m.tbl = %L AND m.old = x.%I', r.tbl, r.col, r.ref, r.col);
  END LOOP;

  -- 5. References without a foreign key.
  UPDATE acl_entries x SET resource_id = m.new FROM _map m
   WHERE m.old = x.resource_id AND m.tbl = CASE x.resource_type WHEN 'basket' THEN 'systems' WHEN 'folder' THEN 'folders' WHEN 'file' THEN 'files' END;
  UPDATE basket_policies x SET scope_id = m.new FROM _map m
   WHERE m.old = x.scope_id AND m.tbl = CASE x.scope_type WHEN 'basket' THEN 'systems' WHEN 'folder' THEN 'folders' END;
  UPDATE rotation_policies x SET scope_id = m.new FROM _map m
   WHERE m.old = x.scope_id AND m.tbl = CASE x.scope_type WHEN 'basket' THEN 'systems' WHEN 'folder' THEN 'folders' WHEN 'org' THEN 'orgs' END;
  UPDATE user_stars x SET ref_id = m.new FROM _map m
   WHERE m.old = x.ref_id AND m.tbl = CASE x.kind WHEN 'basket' THEN 'systems' WHEN 'folder' THEN 'folders' WHEN 'file' THEN 'files' WHEN 'vbasket' THEN 'vbaskets' END;
  UPDATE connectors x SET org_id = m.new FROM _map m WHERE m.tbl = 'orgs' AND m.old = x.org_id;

  -- Ids written inside text and JSON (links, saved lists, basket rules).
  FOR r IN SELECT old, new FROM _map WHERE tbl IN ('systems', 'folders', 'files', 'orgs', 'users', 'vbaskets') LOOP
    UPDATE notifications SET link = replace(link, r.old, r.new) WHERE link LIKE '%' || r.old || '%';
    UPDATE vbaskets SET rules = replace(rules::text, '"' || r.old || '"', '"' || r.new || '"')::jsonb
     WHERE rules::text LIKE '%"' || r.old || '"%';
    UPDATE download_lists SET items = replace(items::text, '"' || r.old || '"', '"' || r.new || '"')::jsonb
     WHERE items::text LIKE '%"' || r.old || '"%';
  END LOOP;

  -- 6. SQL functions that took a UUID take text.
  FOR r IN SELECT p.oid, p.oid::regprocedure::text AS sig FROM pg_proc p
            WHERE p.pronamespace = 'public'::regnamespace AND 'uuid'::regtype::oid = ANY(p.proargtypes::oid[]) LOOP
    t := regexp_replace(pg_get_functiondef(r.oid), '\muuid\M', 'text', 'gi');
    EXECUTE 'DROP FUNCTION ' || r.sig;
    EXECUTE t;
  END LOOP;

  -- 7. Foreign keys back on.
  FOR r IN SELECT * FROM _fk LOOP
    EXECUTE format('ALTER TABLE %s ADD CONSTRAINT %I %s', r.tbl, r.conname, r.def);
  END LOOP;
END $$;

-- Connector accounts no longer carry the connector id in their address.
UPDATE users SET email = id || '@connector.local' WHERE connector_id IS NOT NULL;
