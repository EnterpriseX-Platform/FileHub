-- Old folder ids that callers of the legacy FileService API still send, mapped to
-- the folder that now holds those files (folder ids were re-keyed). Lookups by
-- the old id (upload parentFolderId, getFiles?parentFolderId) resolve through here.
CREATE TABLE IF NOT EXISTS legacy_folder_aliases (
    legacy_id  TEXT PRIMARY KEY,
    folder_id  TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_legacy_folder_aliases_folder ON legacy_folder_aliases(folder_id);
