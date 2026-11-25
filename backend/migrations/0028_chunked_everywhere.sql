-- One chunked (TUS) upload path for every kind of upload: a new file,
-- a new version of a file, or a file sent through a public upload link.
ALTER TABLE tus_uploads
    ADD COLUMN purpose         TEXT NOT NULL DEFAULT 'file',   -- file | version | link
    ADD COLUMN target_file_id  UUID REFERENCES files(id) ON DELETE CASCADE,
    ADD COLUMN link_token      TEXT,
    ADD COLUMN category_code   TEXT,
    ADD COLUMN fields_meta     TEXT,
    ADD COLUMN note_meta       TEXT,
    ADD COLUMN submitter_name  TEXT,
    ADD COLUMN submitter_email TEXT,
    ADD COLUMN failed_reason   TEXT;
CREATE INDEX idx_tus_link ON tus_uploads(link_token) WHERE link_token IS NOT NULL;
