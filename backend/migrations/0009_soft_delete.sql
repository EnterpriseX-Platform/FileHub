-- Soft delete + Trash bin (TOR: data must be recoverable).
ALTER TABLE files   ADD COLUMN deleted_at TIMESTAMPTZ;
ALTER TABLE folders ADD COLUMN deleted_at TIMESTAMPTZ;

CREATE INDEX idx_files_deleted   ON files(deleted_at)   WHERE deleted_at IS NOT NULL;
CREATE INDEX idx_folders_deleted ON folders(deleted_at) WHERE deleted_at IS NOT NULL;
