-- Older versions keep how they were stored: a basket can start encrypting later,
-- so the current file's flag doesn't say how an earlier version was written.
-- NULL (rows from before this column) = same as the current file.
ALTER TABLE file_versions ADD COLUMN IF NOT EXISTS encrypted BOOLEAN;
