-- Rate-limit counters shared by every FileHub pod (was per-process memory).
-- UNLOGGED: counters are throwaway, so skip the write-ahead log for speed.
CREATE UNLOGGED TABLE rate_hits (
    key          TEXT PRIMARY KEY,
    window_start TIMESTAMPTZ NOT NULL,
    hits         INT NOT NULL
);
CREATE INDEX idx_rate_hits_window ON rate_hits (window_start);
