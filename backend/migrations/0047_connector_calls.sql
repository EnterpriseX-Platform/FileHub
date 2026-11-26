-- 0047 — Every call on the connector routes (/FileService/...), reads included,
-- so an admin can see which system called what. Kept 180 days.
CREATE TABLE IF NOT EXISTS connector_calls (
    id         TEXT PRIMARY KEY DEFAULT fh_cuid(),
    at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    connector  TEXT,
    method     TEXT NOT NULL,
    path       TEXT NOT NULL,
    status     INT NOT NULL,
    page       TEXT,
    client_ip  TEXT,
    ms         INT
);
CREATE INDEX IF NOT EXISTS connector_calls_at ON connector_calls (at DESC);
CREATE INDEX IF NOT EXISTS connector_calls_connector ON connector_calls (connector, at DESC);
