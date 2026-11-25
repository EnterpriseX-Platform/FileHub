-- 0020 — Search that works for Thai (TOR 4.15.14)
--
-- Thai has no spaces between words, so tsvector('simple') only matches whole
-- runs of text. Trigram indexes let ILIKE '%คำ%' find a word anywhere in a
-- file name, field value or extracted document text, fast. pg_trgm ships with
-- PostgreSQL; if the role may not create extensions the indexes are skipped and
-- search still works (slower).
DO $$
BEGIN
  CREATE EXTENSION IF NOT EXISTS pg_trgm;
EXCEPTION WHEN insufficient_privilege THEN
  RAISE NOTICE 'pg_trgm not available — Thai substring search runs without an index';
END $$;

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_trgm') THEN
    EXECUTE 'CREATE INDEX IF NOT EXISTS idx_files_name_trgm ON files USING GIN (name gin_trgm_ops)';
    EXECUTE 'CREATE INDEX IF NOT EXISTS idx_files_meta_trgm ON files USING GIN (metadata gin_trgm_ops)';
    EXECUTE 'CREATE INDEX IF NOT EXISTS idx_file_content_trgm ON file_content USING GIN (content gin_trgm_ops)';
  END IF;
END $$;
