-- Production hardening — indexes that matter under real load and a
-- CHECK constraint to prevent the size_bytes overflow path the audit
-- flagged.  All statements are idempotent (`IF NOT EXISTS` / `NOT VALID`
-- → `VALIDATE` pattern) so re-running this migration is safe.

-- ---------------------------------------------------------------------------
-- Indexes on hot predicate columns that don't have one yet.
-- ---------------------------------------------------------------------------

-- Used by /api/auth/me, the session cleanup job, and the require_session
-- middleware ("is this token still valid?").  Today every request scans
-- the sessions table by token (we already have an index there), but the
-- expiry cleanup that age-rotates rows still scans linearly.
CREATE INDEX IF NOT EXISTS idx_sessions_expires_at
    ON sessions (expires_at);

-- "Files I uploaded" / quota per user is a daily-driver dashboard query.
CREATE INDEX IF NOT EXISTS idx_files_created_by
    ON files (created_by)
    WHERE deleted_at IS NULL;

-- The owner display field is what the table-view's "Owner" filter sorts on.
CREATE INDEX IF NOT EXISTS idx_files_owner
    ON files (owner)
    WHERE deleted_at IS NULL;

-- file_versions.uploaded_by powers the version-history sidebar.
CREATE INDEX IF NOT EXISTS idx_file_versions_uploaded_by
    ON file_versions (uploaded_by);

-- ---------------------------------------------------------------------------
-- Defensive CHECK on file size — block i64::MAX rows that would overflow
-- the quota SUMs.
-- ---------------------------------------------------------------------------

-- NOT VALID first so existing rows aren't re-checked; VALIDATE separately
-- once we're confident.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'files_size_bytes_nonneg'
    ) THEN
        ALTER TABLE files
            ADD CONSTRAINT files_size_bytes_nonneg CHECK (size_bytes >= 0) NOT VALID;
        ALTER TABLE files VALIDATE CONSTRAINT files_size_bytes_nonneg;
    END IF;
END$$;
