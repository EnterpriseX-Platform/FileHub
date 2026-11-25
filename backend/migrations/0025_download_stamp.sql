-- 0025 — Stamp downloads: PDF (and Office, delivered as PDF) downloads carry
-- who downloaded, when, the file id and a copy code on every page.
-- NULL = inherit; Vault baskets always stamp.
ALTER TABLE basket_policies ADD COLUMN IF NOT EXISTS stamp_downloads BOOLEAN;
