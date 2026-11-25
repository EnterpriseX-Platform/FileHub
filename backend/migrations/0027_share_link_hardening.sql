-- 0027 — Public link hardening
--   password (argon2 hash), most downloads, revoke, access counting/log
ALTER TABLE share_links ADD COLUMN IF NOT EXISTS password_hash  TEXT;
ALTER TABLE share_links ADD COLUMN IF NOT EXISTS max_downloads  INT;
ALTER TABLE share_links ADD COLUMN IF NOT EXISTS download_count INT NOT NULL DEFAULT 0;
ALTER TABLE share_links ADD COLUMN IF NOT EXISTS revoked_at     TIMESTAMPTZ;
ALTER TABLE share_links ADD COLUMN IF NOT EXISTS last_access_at TIMESTAMPTZ;
