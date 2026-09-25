-- Branding / locale settings are per workspace, not baked into the product.
-- Empty value = product default.  A deployment sets its own organisation name,
-- accent colour, portal link, e-mail hint and fiscal-year convention here
-- (Settings → General), so the same build serves any organisation.
INSERT INTO workspace_config (key, value, description) VALUES
  ('org_name',                 '',                'Organisation shown under the product name (login page, sidebar)'),
  ('brand_accent',             '',                'Accent colour (#rrggbb); empty = product default'),
  ('login_email_placeholder',  'you@example.com', 'Hint shown in the login e-mail field'),
  ('portal_url',               '',                'Link back to the organisation portal (access-denied page); empty = hidden'),
  ('portal_label',             'Back to portal',  'Label for the portal link'),
  ('access_help',              'Ask your administrator to grant you access.', 'Shown when a signed-in user has no File Hub role'),
  ('fiscal_year_start_month',  '0',               'Month (1-12) the fiscal year starts; 0 = no fiscal-year auto tag'),
  ('fiscal_year_era',          'CE',              'Year numbering for the fiscal-year tag: CE or BE (Buddhist Era, +543)')
ON CONFLICT (key) DO NOTHING;

-- The demo tenant slug was the product default; start neutral.
UPDATE workspace_config SET value = '' WHERE key = 'workspace_name' AND value = 'acme.go.th';
