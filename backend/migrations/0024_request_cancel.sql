-- Allow a requester to withdraw their own pending request. A cancelled request
-- leaves reviewers' inboxes and reads as "withdrawn"; its anchor workflow is
-- moved to a terminal 'Cancelled' state so no decision is expected.
ALTER TABLE requests ADD COLUMN IF NOT EXISTS cancelled_at TIMESTAMPTZ;
