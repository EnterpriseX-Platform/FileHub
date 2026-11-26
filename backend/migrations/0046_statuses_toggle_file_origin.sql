-- 0046 — Statuses can be switched off per basket, and every upload keeps a
-- record of where it came from.

-- Off: files keep their stored status, but it is not shown, can't be changed,
-- doesn't lock content and doesn't put files in "Awaiting my review".
ALTER TABLE systems ADD COLUMN IF NOT EXISTS use_statuses BOOLEAN NOT NULL DEFAULT true;

-- Where a file came from. Written once when the file is uploaded; never edited.
CREATE TABLE IF NOT EXISTS file_origin (
    file_id        TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    channel        TEXT NOT NULL,            -- web | api | connector
    connector_name TEXT,
    source_system  TEXT,                     -- systemCode sent by the caller
    source_user    TEXT,                     -- the person who pressed send in the source system
    org_code       TEXT,
    ref_no         TEXT,                     -- request / document number in the source system
    screen         TEXT,                     -- source screen code or page
    client_ip      TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS file_origin_source_user ON file_origin (source_user);
CREATE INDEX IF NOT EXISTS file_origin_ref_no ON file_origin (ref_no);
CREATE INDEX IF NOT EXISTS file_origin_org ON file_origin (org_code);

CREATE OR REPLACE FUNCTION fh_file_origin_readonly() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'file origin is read-only'; END $$;
DROP TRIGGER IF EXISTS file_origin_readonly ON file_origin;
CREATE TRIGGER file_origin_readonly BEFORE UPDATE ON file_origin FOR EACH ROW EXECUTE FUNCTION fh_file_origin_readonly();
