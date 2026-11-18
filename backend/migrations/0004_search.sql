-- Full-text search index for files (TOR 4.15.14 — search across documents).
--
-- We use the 'simple' configuration because it tokenises on whitespace/word
-- boundaries without language-specific stemming — that means it cooperates
-- with the Thai data we have today while still indexing Latin tokens.
-- Substring/ILIKE search is layered on top in the handler to cover Thai
-- queries that do not split on whitespace.
ALTER TABLE files
    ADD COLUMN search_tsv tsvector
    GENERATED ALWAYS AS (
        to_tsvector(
            'simple',
            coalesce(name, '')      || ' ' ||
            coalesce(project, '')   || ' ' ||
            coalesce(owner, '')     || ' ' ||
            coalesce(status, '')    || ' ' ||
            coalesce(file_type, '') || ' ' ||
            coalesce(tags, '')
        )
    ) STORED;

CREATE INDEX idx_files_search ON files USING GIN (search_tsv);

-- Trigram index for fast substring matching (Thai, partial names).
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX idx_files_name_trgm ON files USING GIN (name gin_trgm_ops);
CREATE INDEX idx_files_tags_trgm ON files USING GIN (tags gin_trgm_ops);
