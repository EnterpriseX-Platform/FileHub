-- Object lock (WORM) and webhooks — MinIO-style features. (Lifecycle already
-- exists as rotation policies: keep N versions, archive/delete after N days.)

-- Per-file retention. COMPLIANCE: nobody can delete before retain_until, nor
-- shorten it. GOVERNANCE: same, but an admin may override. legal_hold blocks
-- deletion until lifted, whatever the date.
CREATE TABLE IF NOT EXISTS object_locks (
    file_id      UUID PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    mode         TEXT CHECK (mode IN ('GOVERNANCE', 'COMPLIANCE')),
    retain_until TIMESTAMPTZ,
    legal_hold   BOOLEAN NOT NULL DEFAULT false,
    set_by       TEXT REFERENCES users(id) ON DELETE SET NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Basket/folder default: lock every new file for N days.
ALTER TABLE basket_policies
    ADD COLUMN IF NOT EXISTS lock_days INT,
    ADD COLUMN IF NOT EXISTS lock_mode TEXT CHECK (lock_mode IN ('GOVERNANCE', 'COMPLIANCE'));

-- Webhooks: POST a signed JSON event to `url` when something happens in a
-- basket (or anywhere, when system_id is NULL).
CREATE TABLE IF NOT EXISTS webhooks (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL,
    url        TEXT NOT NULL,
    secret     TEXT NOT NULL,
    system_id  TEXT REFERENCES systems(id) ON DELETE CASCADE,
    events     TEXT[] NOT NULL,
    active     BOOLEAN NOT NULL DEFAULT true,
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Outbox: events are written here in the request, delivered by a worker with
-- retries, so a slow receiver never slows an upload.
CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id           UUID PRIMARY KEY,
    webhook_id   UUID NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event        TEXT NOT NULL,
    payload      JSONB NOT NULL,
    attempts     INT NOT NULL DEFAULT 0,
    next_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at TIMESTAMPTZ,
    last_status  INT,
    last_error   TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_webhook_due ON webhook_deliveries (next_at) WHERE delivered_at IS NULL;
