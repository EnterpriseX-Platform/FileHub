-- A basket can be a folder mounted into the pod (Docker volume, NFS, SMB).
ALTER TABLE systems ADD COLUMN IF NOT EXISTS mount_path TEXT;
ALTER TABLE systems ADD COLUMN IF NOT EXISTS mount_scanned_at TIMESTAMPTZ;
CREATE INDEX IF NOT EXISTS files_mount_key ON files (system_id) WHERE object_key LIKE 'mount:%';
