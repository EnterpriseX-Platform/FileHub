-- 0020_workflow_engine.sql — configurable workflow (TOR Annex A: ระบบงานเอกสาร
-- ตามกระบวนงาน). Adds ordered (sequential) vs parallel routing, named steps,
-- reusable no-code templates, and (via handlers) send-back to a previous step.

-- Routing mode + optional template link on each running workflow.
ALTER TABLE file_workflows ADD COLUMN order_mode  TEXT NOT NULL DEFAULT 'parallel'; -- sequential | parallel
ALTER TABLE file_workflows ADD COLUMN template_id TEXT;  -- references workflow_templates.id

-- Human label for a step (from the template or the start request).
ALTER TABLE workflow_steps ADD COLUMN name TEXT;

-- Reusable, admin-defined flow templates (no-code): an ordered list of steps in
-- `steps` JSONB, each { name, reviewer_id } — expanded into workflow_steps when
-- a workflow is started from the template.
CREATE TABLE workflow_templates (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    order_mode  TEXT NOT NULL DEFAULT 'sequential',
    steps       JSONB NOT NULL DEFAULT '[]',
    created_by  TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
