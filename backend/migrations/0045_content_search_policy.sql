-- Basket policy: index the text inside files so search finds it (default on).
ALTER TABLE basket_policies ADD COLUMN IF NOT EXISTS content_search BOOLEAN;
