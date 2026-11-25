-- A connector may accept callers that send no registered token: only for
-- deployments where /FileService is reachable from inside the cluster alone.
ALTER TABLE connectors ADD COLUMN IF NOT EXISTS trust_callers BOOLEAN NOT NULL DEFAULT false;
