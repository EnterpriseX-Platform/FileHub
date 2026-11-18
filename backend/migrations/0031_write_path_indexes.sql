-- Every upload updates every index on `files`; at thousands of concurrent
-- uploads Postgres spent its time there. These three were never used:
--   idx_files_meta_gin  GIN on fh_meta(metadata) — lookups use ->> (no GIN help)
--   idx_files_owner     owner search uses the trigram index
--   idx_files_system    covered by idx_files_list / idx_files_category (system_id first)
DROP INDEX IF EXISTS idx_files_meta_gin;
DROP INDEX IF EXISTS idx_files_owner;
DROP INDEX IF EXISTS idx_files_system;
