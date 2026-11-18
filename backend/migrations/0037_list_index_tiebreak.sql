-- File lists sort by (modified_at DESC, id DESC); with id in the key the index
-- returns rows already in that order, so a page reads LIMIT rows instead of
-- sorting the whole folder.
CREATE INDEX IF NOT EXISTS idx_files_list2 ON files (system_id, folder_id, modified_at DESC, id DESC) INCLUDE (created_by) WHERE deleted_at IS NULL;
DROP INDEX IF EXISTS idx_files_list;
ANALYZE files;
