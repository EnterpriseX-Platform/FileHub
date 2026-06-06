-- P1 feature tables.  Workflow + comments + notifications + thumbnails are
-- transaction-like → UUIDv7 PKs.  Their FK columns to system tables use TEXT.

CREATE TABLE file_workflows (
    id          UUID PRIMARY KEY,
    file_id     UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    state       TEXT NOT NULL DEFAULT 'Draft',
    note        TEXT,
    created_by  TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_workflows_file ON file_workflows(file_id);

CREATE TABLE workflow_steps (
    id            UUID PRIMARY KEY,
    workflow_id   UUID NOT NULL REFERENCES file_workflows(id) ON DELETE CASCADE,
    reviewer_id   TEXT REFERENCES users(id) ON DELETE SET NULL,
    decision      TEXT,
    note          TEXT,
    sequence      INT  NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at    TIMESTAMPTZ
);
CREATE INDEX idx_workflow_steps_workflow ON workflow_steps(workflow_id);
CREATE INDEX idx_workflow_steps_reviewer ON workflow_steps(reviewer_id);

CREATE TABLE file_content (
    file_id     UUID PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    content     TEXT NOT NULL,
    extracted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    content_tsv  tsvector GENERATED ALWAYS AS (to_tsvector('simple', content)) STORED
);
CREATE INDEX idx_file_content_tsv ON file_content USING GIN (content_tsv);

CREATE TABLE thumbnails (
    file_id     UUID PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    width       INT  NOT NULL,
    height      INT  NOT NULL,
    mime        TEXT NOT NULL DEFAULT 'image/webp',
    data        BYTEA NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE notifications (
    id          UUID PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    title       TEXT NOT NULL,
    body        TEXT,
    link        TEXT,
    read_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_notifications_user_unread ON notifications(user_id) WHERE read_at IS NULL;
CREATE INDEX idx_notifications_user_time  ON notifications(user_id, created_at DESC);

CREATE TABLE file_comments (
    id          UUID PRIMARY KEY,
    file_id     UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    parent_id   UUID REFERENCES file_comments(id) ON DELETE CASCADE,
    body        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_file_comments_file ON file_comments(file_id, created_at);
