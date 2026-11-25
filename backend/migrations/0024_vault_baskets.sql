-- 0024 — Vault baskets: a basket kind for the most sensitive documents.
--   * every file is encrypted (policy forced, cannot be turned off below it)
--   * only people given access explicitly — no role fallback, never empty
--   * no public links; upload links need sign-in
--   * every view, preview and download is written to the activity log
ALTER TABLE systems ADD COLUMN IF NOT EXISTS vault BOOLEAN NOT NULL DEFAULT false;
