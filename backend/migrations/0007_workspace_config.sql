-- Workspace config — a flat key/value store for tunables that historically
-- lived as hard-coded constants in the frontend (storage quota, branding,
-- feature flags). Read by the dashboard so admins can change them via SQL
-- without a redeploy.
CREATE TABLE workspace_config (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    description TEXT,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 36 TiB expressed in bytes (36 * 1024^4). The dashboard divides by 1024^4
-- to show "TB" headers, so this preserves the original "36 TB quota" copy
-- while making it editable from SQL.
INSERT INTO workspace_config (key, value, description) VALUES
  ('storage_quota_bytes', '39582418599936', 'Total storage quota in bytes (36 TiB default)'),
  ('workspace_name',      'acme.go.th',     'Tenant slug shown under the workspace logo'),
  ('workspace_display',   'File Hub',       'Display name shown in the sidebar header');
