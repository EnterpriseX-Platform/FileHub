-- 0041 — folder encryption method
--
-- NULL = inherit from the parent folder / basket (default: symmetric, the
-- server's AES-256 key). 'asymmetric' = each file gets its own data key,
-- wrapped with the server's RSA public key; the private key lives outside the
-- database (STORAGE_RSA_PRIVATE_KEY_FILE).
ALTER TABLE basket_policies ADD COLUMN IF NOT EXISTS encrypt_mode TEXT;
