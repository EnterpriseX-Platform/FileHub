-- ---------------------------------------------------------------------------
-- Requests — form/document submissions that flow through org approvals.
--
-- A request is a thin overlay on top of the existing approval engine: it
-- anchors to a *document* (either one the user attaches, or a summary doc the
-- backend generates from the form) and reuses `file_workflows` verbatim for
-- routing, decisions, send-back, notifications and audit. This table only adds
-- the form semantics (kind + field values + AI summary) on top.
-- See backend/src/requests.rs.
-- ---------------------------------------------------------------------------

-- Shared area that holds request anchor documents. It must be `shared` (not a
-- personal drive) so every assigned reviewer can read the attached document —
-- a personal-drive anchor would 403 the reviewers. Idempotent.
INSERT INTO systems (id, name, tone, bucket, status, description, system_type)
VALUES ('sys_requests', 'Requests', 'indigo', 'requests-store', 'live',
        'Form and document requests routed for approval', 'shared')
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS requests (
    id          UUID PRIMARY KEY,
    kind        TEXT NOT NULL,                          -- expense | it | document | leave
    title       TEXT NOT NULL,
    form_data   JSONB NOT NULL DEFAULT '{}',            -- filled field values
    amount      DOUBLE PRECISION,                       -- optional; drives expense routing
    file_id     UUID REFERENCES files(id) ON DELETE SET NULL,   -- anchor document
    system_id   TEXT NOT NULL DEFAULT 'sys_requests' REFERENCES systems(id),
    org_id      TEXT,
    ai_summary  TEXT,                                   -- one-liner written for the approver
    created_by  TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_requests_creator ON requests(created_by);
CREATE INDEX IF NOT EXISTS idx_requests_file    ON requests(file_id);
CREATE INDEX IF NOT EXISTS idx_requests_created ON requests(created_at DESC);
