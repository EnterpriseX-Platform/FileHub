-- Phase U — Collabora Online integration.
--
-- Pick the office-document viewer at runtime so admins can flip between
-- `collabora` (interactive WebSocket viewer) and `pdf` (server-rendered
-- PDF) without redeploying.  Disabled = no inline viewer at all (forces
-- Download), useful when Collabora is down.
INSERT INTO workspace_config (key, value, description) VALUES
  ('office_viewer', 'pdf',
   'Office-doc viewer: pdf | collabora | disabled.  Collabora requires the docker-compose service to be up.'),
  ('collabora_url', 'http://localhost:9980',
   'Public URL of the Collabora Online server as the BROWSER sees it (not the docker-internal hostname).')
ON CONFLICT (key) DO NOTHING;
