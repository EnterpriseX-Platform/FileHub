-- Readable seed IDs for system tables (`sys_*`, `org_*`).  Transaction tables
-- (files, activity, permissions) still use fixed UUIDv7-shaped literals so
-- tests can reference them as constants.

INSERT INTO systems (id, name, tone, bucket, status, description) VALUES
  ('sys_hr',    'HR System',     'rose',    'hr-emp-files',    'live', 'Employee files, contracts, performance'),
  ('sys_fin',   'Finance',       'cyan',    'fin-invoices',    'live', 'Budgets, invoices, payroll'),
  ('sys_crm',   'CRM',           'emerald', 'crm-customers',   'live', 'Customer records, sales pipeline'),
  ('sys_dms',   'Document Mgmt', 'amber',   'dms-archive',     'live', 'General document repository'),
  ('sys_legal', 'Legal',         'violet',  'legal-contracts', 'live', 'Contracts, NDAs, legal templates'),
  ('sys_it',    'IT Operations', 'fuchsia', 'it-ops',          'live', 'Runbooks, configs, backups'),
  ('sys_proc',  'Procurement',   'slate',   'proc-vendors',    'live', 'Vendor management, RFPs');

INSERT INTO orgs (id, system_id, name, code, tier, owner, tags) VALUES
  ('org_phattana',     'sys_hr',    'สำนัก พัฒนาฯ',          'HR-DEV-001', 'Department', 'Anong K.',  '["hq","central"]'),
  ('org_purchase',     'sys_hr',    'กลุ่ม จัดซื้อ',         'HR-PRC-014', 'Group',      'Pat S.',    '["procurement"]'),
  ('org_account',      'sys_hr',    'ฝ่าย บัญชี',            'HR-ACC-021', 'Section',    'Pat S.',    '["finance"]'),
  ('org_budget',       'sys_hr',    'สำนัก งบประมาณ',       'HR-BUD-007', 'Department', 'Wisanu T.', '["budget"]'),
  ('org_hr_central',   'sys_hr',    'HR ส่วนกลาง',          'HR-CTL-000', 'HQ',         'HR Admin',  '["hq","central"]'),
  ('org_resource',     'sys_hr',    'ฝ่าย ทรัพยากร',         'HR-RES-033', 'Section',    'Krit M.',   '["hr"]'),
  ('org_recruit',      'sys_hr',    'กลุ่ม สรรหา',           'HR-REC-018', 'Group',      'Anong K.',  '["hr","recruit"]'),
  ('org_audit',        'sys_hr',    'สำนัก ตรวจสอบ',         'HR-AUD-002', 'Department', 'Wisanu T.', '["audit"]'),
  ('org_train',        'sys_hr',    'กลุ่ม ฝึกอบรม',         'HR-TRN-009', 'Group',      'Krit M.',   '["training"]'),
  ('org_welfare',      'sys_hr',    'ฝ่าย สวัสดิการ',        'HR-WEL-027', 'Section',    'HR Admin',  '["benefits"]'),
  ('org_legal_int',    'sys_hr',    'สำนัก กฎหมาย (ภายใน)',  'HR-LEG-004', 'Department', 'Wisanu T.', '["legal"]'),
  ('org_it_support',   'sys_hr',    'ฝ่าย IT สนับสนุน',      'HR-IT-012',  'Section',    'IT Admin',  '["it"]'),
  ('org_fin_ap',       'sys_fin',   'Accounts Payable',      'FIN-AP-001', 'Department', 'Pat S.',    '["finance"]'),
  ('org_fin_ar',       'sys_fin',   'Accounts Receivable',   'FIN-AR-002', 'Department', 'Pat S.',    '["finance"]'),
  ('org_crm_sales',    'sys_crm',   'Sales',                 'CRM-SAL-001','Department', 'Sarah L.',  '["sales"]'),
  ('org_legal_out',    'sys_legal', 'External Counsel',      'LEG-EXT-001','Group',      'Wisanu T.', '["legal","external"]');

INSERT INTO views (id, name, layout, color, pinned, group_by, sort_by, filters) VALUES
  ('vw_q1board',     'Q1 2026 Board',  'board',   '#4f46e5', true,  'status',  'modified_desc', '[{"field":"project","op":"is","value":"Q1-2026"}]'),
  ('vw_needs_review','Needs review',   'table',   '#dc2626', true,  NULL,      'modified_desc', '[{"field":"status","op":"is","value":"Review"}]'),
  ('vw_expiring',    'Expiring soon',  'table',   '#d97706', true,  NULL,      'modified_desc', '[{"field":"expires","op":"within","value":"30d"}]'),
  ('vw_my_uploads',  'My uploads',     'table',   '#16a34a', true,  NULL,      'modified_desc', '[{"field":"owner","op":"is","value":"me"}]'),
  ('vw_photos',      'Photos gallery', 'gallery', '#d97706', false, NULL,      'modified_desc', '[{"field":"file_type","op":"in","value":["img","jpg","png"]}]');

INSERT INTO files (id, name, file_type, size_bytes, system_id, org_id, bucket, object_key, project, status, owner, tags, version, etag) VALUES
  ('00000000-0000-7000-8000-000000002001'::uuid, 'contract-A12.pdf',           'pdf',  2517299, 'sys_hr',    'org_phattana',   'hr-emp-files',    'contracts/2026-Q1/contract-A12.pdf',           'Q1-2026', 'Review',   'Anong K.',    '["legal","signed"]',   4, 'a1f2c84b9e'),
  ('00000000-0000-7000-8000-000000002002'::uuid, 'contract-A13-amendment.pdf', 'pdf',  1887436, 'sys_hr',    'org_phattana',   'hr-emp-files',    'contracts/2026-Q1/contract-A13-amendment.pdf', 'Q1-2026', 'Draft',    'Anong K.',    '["legal","draft"]',    2, 'b3e5d621cc'),
  ('00000000-0000-7000-8000-000000002003'::uuid, 'employee-roster.csv',        'csv',   831488, 'sys_hr',    'org_hr_central', 'hr-emp-files',    'exports/employee-roster.csv',                  'Q1-2026', 'Approved', 'Finance Bot', '["payroll","monthly"]',1, 'c8aa01ec55'),
  ('00000000-0000-7000-8000-000000002004'::uuid, 'onboarding-2026.pdf',        'pdf',  2517299, 'sys_hr',    'org_hr_central', 'hr-emp-files',    'handbook/onboarding-2026.pdf',                 'Onboard', 'Approved', 'HR Admin',    '["hr","signed"]',      6, 'd9bb12fd66'),
  ('00000000-0000-7000-8000-000000002005'::uuid, 'workshop-photos',            'fold',       0, 'sys_hr',    'org_phattana',   'hr-emp-files',    'media/workshop-photos/',                       'Events',  'Active',   'Krit M.',     '["event","training"]', 1, 'e0cc23ge77'),
  ('00000000-0000-7000-8000-000000002006'::uuid, 'NDA-template-v3.docx',       'docx',  348160, 'sys_legal', 'org_legal_int',  'legal-contracts', 'templates/NDA-template-v3.docx',               'Legal',   'Review',   'Wisanu T.',   '["legal","template"]', 3, 'f1dd34hf88'),
  ('00000000-0000-7000-8000-000000002007'::uuid, 'budget-q1-2026.xlsx',        'xlsx', 1153434, 'sys_fin',   'org_fin_ap',     'fin-invoices',    'budgets/2026/budget-q1-2026.xlsx',             'Q1-2026', 'Approved', 'Pat S.',      '["finance","q1"]',     8, '02ee45ig99'),
  ('00000000-0000-7000-8000-000000002008'::uuid, 'policy-handbook-v4.pdf',     'pdf',  1887436, 'sys_hr',    'org_hr_central', 'hr-emp-files',    'handbook/policy-handbook-v4.pdf',              'Policy',  'Approved', 'HR Admin',    '["policy","signed"]',  4, '13ff56jh00'),
  ('00000000-0000-7000-8000-000000002009'::uuid, 'kickoff-call.mp4',           'mp4', 86402764, 'sys_hr',    'org_phattana',   'hr-emp-files',    'media/kickoff-call.mp4',                       'Events',  'Archived', 'Anong K.',    '["video","event"]',    1, '24gg67ki11'),
  ('00000000-0000-7000-8000-00000000200a'::uuid, 'archive-2025.zip',           'zip',440401920, 'sys_hr',    'org_hr_central', 'hr-emp-files',    'archive/archive-2025.zip',                     'Archive', 'Archived', 'System',      '["archive"]',          1, '35hh78lj22'),
  ('00000000-0000-7000-8000-00000000200b'::uuid, 'team-photo.jpg',             'img',  4404019, 'sys_hr',    'org_phattana',   'hr-emp-files',    'media/team-photo.jpg',                         'Events',  'Approved', 'Krit M.',     '["event","photo"]',    1, '46ii89mk33'),
  ('00000000-0000-7000-8000-00000000200c'::uuid, 'vendor-list-2026.xlsx',      'xlsx',  634880, 'sys_proc',  'org_purchase',   'proc-vendors',    'vendors/vendor-list-2026.xlsx',                'Vendor',  'Draft',    'Pat S.',      '["vendor","q1"]',      3, '57jj90nl44');

INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at) VALUES
  ('00000000-0000-7000-8000-000000003001'::uuid, 'Anong K.',    'rose',   'uploaded',                    'contract-A12.pdf',              'pdf',  '00000000-0000-7000-8000-000000002001'::uuid, 'sys_hr',  'org_phattana',   now() - interval '2 minutes'),
  ('00000000-0000-7000-8000-000000003002'::uuid, 'Finance Bot', 'cyan',   'auto-imported 14 files into', 'vendor-invoices/',              'fold', NULL,                                            'sys_fin', 'org_purchase',   now() - interval '18 minutes'),
  ('00000000-0000-7000-8000-000000003003'::uuid, 'Krit M.',     'rose',   'shared',                      'team-photo.jpg',                'img',  '00000000-0000-7000-8000-00000000200b'::uuid, 'sys_hr',  'org_phattana',   now() - interval '1 hours'),
  ('00000000-0000-7000-8000-000000003004'::uuid, 'Pat S.',      'cyan',   'approved',                    'budget-q1-2026.xlsx',           'xlsx', '00000000-0000-7000-8000-000000002007'::uuid, 'sys_fin', 'org_fin_ap',     now() - interval '2 hours'),
  ('00000000-0000-7000-8000-000000003005'::uuid, 'CRM Sync',    'emerald','created export',              'customer-export-2026-01.csv',   'csv',  NULL,                                            'sys_crm', 'org_crm_sales',  now() - interval '3 hours'),
  ('00000000-0000-7000-8000-000000003006'::uuid, 'System',      'rose',   'archived',                    '2025-q4 batch (32 files)',      'fold', NULL,                                            'sys_hr',  'org_hr_central', now() - interval '5 hours');

INSERT INTO permissions (id, file_id, principal, principal_type, role, external) VALUES
  ('00000000-0000-7000-8000-000000005001'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'Anong K. (anong@acme.go.th)',     'user',  'owner',  false),
  ('00000000-0000-7000-8000-000000005002'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'HR Admins',                       'group', 'manage', false),
  ('00000000-0000-7000-8000-000000005003'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'สำนัก พัฒนาฯ team',               'group', 'edit',   false),
  ('00000000-0000-7000-8000-000000005004'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'wisanu@acme.go.th',               'user',  'edit',   false),
  ('00000000-0000-7000-8000-000000005005'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'pat@acme.go.th',                  'user',  'edit',   false),
  ('00000000-0000-7000-8000-000000005006'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'krit@acme.go.th',                 'user',  'view',   false),
  ('00000000-0000-7000-8000-000000005007'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'external (expires 31 Mar 2026)',  'link',  'view',   true);
