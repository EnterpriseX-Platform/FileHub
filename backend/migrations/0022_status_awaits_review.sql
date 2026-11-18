-- 0022 — Which statuses mean "waiting for someone to review".
-- Drives "Awaiting my review": files in such a status that the caller may move
-- with Manage. Set per basket in Settings → Statuses.
ALTER TABLE basket_statuses ADD COLUMN IF NOT EXISTS awaits_review BOOLEAN NOT NULL DEFAULT false;
UPDATE basket_statuses SET awaits_review = true WHERE code = 'Review';
