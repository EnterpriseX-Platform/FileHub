-- Folders (TOR 4.15.4). System table → TEXT/CUID2 PK.
CREATE TABLE folders (
    id          TEXT PRIMARY KEY,
    system_id   TEXT NOT NULL REFERENCES systems(id),
    org_id      TEXT REFERENCES orgs(id),
    parent_id   TEXT REFERENCES folders(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    color       TEXT,
    owner       TEXT NOT NULL,
    encrypted   BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_folders_system ON folders(system_id);
CREATE INDEX idx_folders_parent ON folders(parent_id);
CREATE INDEX idx_folders_org    ON folders(org_id);

ALTER TABLE files ADD COLUMN folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL;
ALTER TABLE files ADD COLUMN encrypted BOOLEAN NOT NULL DEFAULT false;
CREATE INDEX idx_files_folder ON files(folder_id);

INSERT INTO folders (id, system_id, org_id, name, color, owner, encrypted) VALUES
  ('fld_contracts2026', 'sys_hr',    'org_phattana',   'Contracts 2026', '#dc2626', 'Anong K.',  false),
  ('fld_handbooks',     'sys_hr',    'org_hr_central', 'Handbooks',      '#16a34a', 'HR Admin',  false),
  ('fld_budgets',       'sys_fin',   'org_fin_ap',     'Budgets',        '#0ea5e9', 'Pat S.',    false),
  ('fld_confidential',  'sys_legal', 'org_legal_int',  'Confidential',   '#7c3aed', 'Wisanu T.', true);
