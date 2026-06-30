-- 0017_checkout.sql — document check-out / check-in locking (MEA TOR 5.3.8.4-5).
--
-- A checked-out file is locked to one editor: others may view but not edit or
-- check out, until the holder checks it back in (or a stale lock is reaped by
-- the rotation worker). Lock state lives on the files row; NULL = not locked.
ALTER TABLE files ADD COLUMN checked_out_by      TEXT;
ALTER TABLE files ADD COLUMN checked_out_by_name TEXT;
ALTER TABLE files ADD COLUMN checked_out_at      TIMESTAMPTZ;
