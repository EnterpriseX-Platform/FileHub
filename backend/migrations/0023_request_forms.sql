-- ---------------------------------------------------------------------------
-- Request forms — admin-authored request types (the Form Designer's storage).
--
-- Replaces the four hardcoded form schemas: each row is a request type an admin
-- built in the Admin Console — its fields and the workflow template that routes
-- it for approval. The Everyday "New request" flow and the AI intake read these
-- at runtime; changing a form needs no code change or redeploy.
--
-- The four defaults are seeded (non-destructively) at startup by
-- requests::bootstrap_request_forms, NOT here, because they link to
-- workflow_templates whose steps reference seed reviewer users that only exist
-- after user bootstrap. Seeding there also means an admin's edits are never
-- clobbered on reboot (INSERT ... ON CONFLICT DO NOTHING).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS request_forms (
    id           TEXT PRIMARY KEY,                 -- 'expense' … or form_<cuid> for new ones
    name_en      TEXT NOT NULL,
    name_th      TEXT NOT NULL,
    description  TEXT,
    icon         TEXT NOT NULL DEFAULT 'generic',  -- glyph key (expense|it|document|leave|generic)
    color        TEXT NOT NULL DEFAULT 'indigo',   -- design-token color name
    fields       JSONB NOT NULL DEFAULT '[]',      -- [{key,label_en,label_th,type,required,options?}]
    template_id  TEXT REFERENCES workflow_templates(id) ON DELETE SET NULL,  -- the approval route
    active       BOOLEAN NOT NULL DEFAULT true,
    sort_order   INT NOT NULL DEFAULT 0,
    created_by   TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_request_forms_active ON request_forms(active, sort_order);
