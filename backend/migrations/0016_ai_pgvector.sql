-- 0016_ai_pgvector.sql — AI-native foundation.
--
-- Adds pgvector + the tables the AI enrichment pipeline writes:
--   chunks            : text chunks of a file (carry page/offset for citations)
--   chunk_embeddings  : the embedding vector per chunk (HNSW cosine index)
--   file_ai           : 1:1 per-file AI summary / tags / entities / sensitivity
--   ai_jobs           : enrichment queue the background worker drains
--   ai_usage          : token/model metering (-> ONEWEB billing later)
--
-- Embedding dimension is FIXED at 768 (the local-first default,
-- nomic-embed-text). pgvector columns are dimension-locked, so changing the
-- model to a different dim requires a follow-up migration + re-embed; the
-- backend logs a warning at boot if AI_EMBED_DIM != 768. See FILEHUB_AI_NATIVE_SPEC.md §8 D-EMB.
--
-- ID strategy follows 0001_init: these are transaction-like tables, so PKs are
-- UUID (UUIDv7 supplied by the app for insert-order index locality), except
-- file_ai whose PK *is* file_id (1:1 with files).

CREATE EXTENSION IF NOT EXISTS vector;

-- One row per text chunk of a file version.
CREATE TABLE chunks (
    id          UUID PRIMARY KEY,
    file_id     UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    version_id  UUID REFERENCES file_versions(id) ON DELETE CASCADE,
    seq         INT  NOT NULL,
    content     TEXT NOT NULL,
    page        INT,
    char_start  INT,
    char_end    INT,
    token_count INT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX chunks_file_idx ON chunks (file_id);

-- The embedding vector for a chunk. Separate table so a re-embed (model change)
-- can TRUNCATE/rebuild without touching chunk text, and so the HNSW index is
-- isolated. model records which embedding model produced it (mismatch detection).
CREATE TABLE chunk_embeddings (
    chunk_id  UUID PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
    embedding vector(768) NOT NULL,
    model     TEXT NOT NULL
);
CREATE INDEX chunk_embeddings_hnsw
    ON chunk_embeddings USING hnsw (embedding vector_cosine_ops);

-- 1:1 per-file AI understanding, written by the enrichment worker.
CREATE TABLE file_ai (
    file_id     UUID PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    summary     TEXT,
    tags        TEXT[] NOT NULL DEFAULT '{}',
    entities    JSONB,
    language    TEXT,
    sensitivity TEXT NOT NULL DEFAULT 'none',   -- none | pii | confidential
    model       TEXT,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Enrichment queue. The worker claims a queued row, runs the pipeline
-- (extract -> chunk -> embed -> summarise), and marks it done/failed. Retriable.
CREATE TABLE ai_jobs (
    id         UUID PRIMARY KEY,
    file_id    UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL DEFAULT 'enrich',
    status     TEXT NOT NULL DEFAULT 'queued',  -- queued | running | done | failed
    attempts   INT  NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Partial index so the worker's "next queued job" scan stays cheap as the
-- table accumulates completed rows.
CREATE INDEX ai_jobs_pending_idx ON ai_jobs (created_at) WHERE status IN ('queued', 'failed');

-- AI usage metering -> ONEWEB billing. One row per billable AI op.
CREATE TABLE ai_usage (
    id            UUID PRIMARY KEY,
    user_id       TEXT,
    system_id     TEXT,
    op            TEXT NOT NULL,                -- embed | summarize | ask | ...
    model         TEXT,
    input_tokens  INT,
    output_tokens INT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ai_usage_created_idx ON ai_usage (created_at);
