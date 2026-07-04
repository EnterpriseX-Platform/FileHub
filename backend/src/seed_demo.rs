//! Demo collaboration data — the rows that make FileHub read like a workspace a
//! real team uses every day rather than an empty reference implementation.
//!
//! Why this lives in Rust and not a migration:
//!   * `sqlx::migrate!` checksums every applied migration, so editing
//!     `0002_seed.sql` to add this would make an already-migrated database
//!     refuse to boot ("migration … was modified").
//!   * Comments / workflows / notifications all FK to `users.id`, and migrations
//!     run *before* `bootstrap_seed_users()` — at migration time the users table
//!     is still empty. Seeding here, right after the users exist, sidesteps both
//!     problems and works on a fresh *and* an already-populated dev database.
//!
//! Idempotency + freshness: every row uses a fixed, recognizable seed UUID
//! (groups `…0030xx` activity, `…0060xx` comments, `…0070xx` workflows/steps,
//! `…0080xx` notifications, `…0090xx` versions, `…0051xx` extra permissions).
//! We DELETE those ids and re-INSERT with `now()`-relative timestamps on every
//! boot, so the demo always looks recent and never duplicates. Runtime rows use
//! UUIDv7 ids that can't collide with these literals, so genuine user activity
//! is left untouched.
//!
//! Seeding is best-effort: a failure here is logged and swallowed by the caller
//! so it can never block startup (see `state.rs`).

use sqlx::PgPool;

/// Seed the demo collaboration data. Safe to call on every boot.
pub async fn bootstrap_demo_data(db: &PgPool) -> anyhow::Result<()> {
    // Guard: the collaborator users must exist (bootstrap_seed_users runs just
    // before us). If they somehow don't, skip rather than trip a FK and abort.
    let have_collaborators: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = 'usr_pat')",
    )
    .fetch_one(db)
    .await?;
    if !have_collaborators {
        tracing::warn!("demo seed skipped: collaborator users missing");
        return Ok(());
    }

    let mut tx = db.begin().await?;

    // ---- Activity feed -----------------------------------------------------
    // Supersede the six frozen, partly-bot rows from 0002 with human actors and
    // timestamps that age from "just now" outward.
    sqlx::query("DELETE FROM activity WHERE id::text LIKE '00000000-0000-7000-8000-000000003%'")
        .execute(&mut *tx).await?;
    sqlx::query(r#"
        INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id, system_id, org_id, created_at) VALUES
          ('00000000-0000-7000-8000-000000003101'::uuid, 'Anong K.',     'rose',    'uploaded a new redline of', 'contract-A12.pdf',            'pdf',  '00000000-0000-7000-8000-000000002001'::uuid, 'sys_hr',    'org_phattana',   now() - interval '20 seconds'),
          ('00000000-0000-7000-8000-000000003102'::uuid, 'Pat S.',       'cyan',    'commented on',              'budget-q1-2026.xlsx',         'xlsx', '00000000-0000-7000-8000-000000002007'::uuid, 'sys_fin',   'org_fin_ap',     now() - interval '4 minutes'),
          ('00000000-0000-7000-8000-000000003103'::uuid, 'Wisanu T.',    'violet',  'approved the review on',    'contract-A12.pdf',            'pdf',  '00000000-0000-7000-8000-000000002001'::uuid, 'sys_hr',    'org_phattana',   now() - interval '2 hours'),
          ('00000000-0000-7000-8000-000000003104'::uuid, 'Sarah L.',     'emerald', 'shared',                    'budget-q1-2026.xlsx',         'xlsx', '00000000-0000-7000-8000-000000002007'::uuid, 'sys_fin',   'org_crm_sales',  now() - interval '2 hours'),
          ('00000000-0000-7000-8000-000000003105'::uuid, 'Krit M.',      'amber',   'requested changes on',      'NDA-template-v3.docx',        'docx', '00000000-0000-7000-8000-000000002006'::uuid, 'sys_legal', 'org_legal_int',  now() - interval '3 hours'),
          ('00000000-0000-7000-8000-000000003106'::uuid, 'Anong K.',     'rose',    'submitted for review',      'contract-A12.pdf',            'pdf',  '00000000-0000-7000-8000-000000002001'::uuid, 'sys_hr',    'org_phattana',   now() - interval '8 hours'),
          ('00000000-0000-7000-8000-000000003107'::uuid, 'Pat S.',       'cyan',    'uploaded',                  'vendor-list-2026.xlsx',       'xlsx', '00000000-0000-7000-8000-00000000200c'::uuid, 'sys_proc',  'org_purchase',   now() - interval '5 hours'),
          ('00000000-0000-7000-8000-000000003108'::uuid, 'Krit M.',      'amber',   'added comments to',         'onboarding-2026.pdf',         'pdf',  '00000000-0000-7000-8000-000000002004'::uuid, 'sys_hr',    'org_hr_central', now() - interval '22 hours'),
          ('00000000-0000-7000-8000-000000003109'::uuid, 'Anong K.',     'rose',    'approved',                  'onboarding-2026.pdf',         'pdf',  '00000000-0000-7000-8000-000000002004'::uuid, 'sys_hr',    'org_hr_central', now() - interval '20 hours'),
          ('00000000-0000-7000-8000-00000000310a'::uuid, 'Sarah L.',     'emerald', 'created an export from',    'customer-export-2026-02.csv', 'csv',  NULL,                                         'sys_crm',   'org_crm_sales',  now() - interval '1 day'),
          ('00000000-0000-7000-8000-00000000310b'::uuid, 'Pat S.',       'cyan',    'completed approval on',     'budget-q1-2026.xlsx',         'xlsx', '00000000-0000-7000-8000-000000002007'::uuid, 'sys_fin',   'org_fin_ap',     now() - interval '1 day'),
          ('00000000-0000-7000-8000-00000000310c'::uuid, 'Krit M.',      'amber',   'uploaded 6 photos to',      'workshop-photos',             'fold', '00000000-0000-7000-8000-000000002005'::uuid, 'sys_hr',    'org_phattana',   now() - interval '2 days'),
          ('00000000-0000-7000-8000-00000000310d'::uuid, 'Finance Sync', 'cyan',    'auto-imported 14 invoices into', 'vendor-invoices/',       'fold', NULL,                                         'sys_fin',   'org_fin_ap',     now() - interval '18 minutes')
    "#).execute(&mut *tx).await?;

    // ---- Comment threads ---------------------------------------------------
    // Parents must precede their replies (parent_id FK). Two threads carry a
    // reply chain so the UI shows back-and-forth, not just isolated notes.
    sqlx::query("DELETE FROM file_comments WHERE id::text LIKE '00000000-0000-7000-8000-000000006%'")
        .execute(&mut *tx).await?;
    sqlx::query(r#"
        INSERT INTO file_comments (id, file_id, user_id, parent_id, body, created_at) VALUES
          -- contract-A12.pdf: legal + finance review thread
          ('00000000-0000-7000-8000-000000006101'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'usr_wisanu', NULL,                                           'Flagged the indemnification clause 4.2 — we need legal to confirm the liability cap before this goes out.', now() - interval '5 hours'),
          ('00000000-0000-7000-8000-000000006102'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'usr_pat',    '00000000-0000-7000-8000-000000006101'::uuid,   'Agreed. Payment terms on p.2 also do not match our standard net-30 window.', now() - interval '4 hours'),
          ('00000000-0000-7000-8000-000000006103'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'usr_anong',  NULL,                                           'Thanks both — pushing redline v5 with the cap and the net-30 fix before end of day.', now() - interval '28 minutes'),
          -- budget-q1-2026.xlsx: finance sign-off
          ('00000000-0000-7000-8000-000000006104'::uuid, '00000000-0000-7000-8000-000000002007'::uuid, 'usr_pat',    NULL,                                           'Q1 projections updated with the revised headcount plan. Numbers reconcile to the ledger.', now() - interval '6 hours'),
          ('00000000-0000-7000-8000-000000006105'::uuid, '00000000-0000-7000-8000-000000002007'::uuid, 'usr_sarah',  NULL,                                           'Looks good from the sales side — pipeline assumptions match our forecast.', now() - interval '2 hours'),
          -- NDA-template-v3.docx: legal template discussion
          ('00000000-0000-7000-8000-000000006106'::uuid, '00000000-0000-7000-8000-000000002006'::uuid, 'usr_wisanu', NULL,                                           'Tightened the confidentiality term to 3 years to match the new policy.', now() - interval '1 day'),
          ('00000000-0000-7000-8000-000000006107'::uuid, '00000000-0000-7000-8000-000000002006'::uuid, 'usr_krit',   NULL,                                           'Can we add a carve-out for already-public information? HR keeps getting asked about it.', now() - interval '3 hours'),
          -- onboarding-2026.pdf: HR handbook
          ('00000000-0000-7000-8000-000000006108'::uuid, '00000000-0000-7000-8000-000000002004'::uuid, 'usr_krit',   NULL,                                           'Added the new benefits section and the IT setup checklist on p.7.', now() - interval '2 days'),
          ('00000000-0000-7000-8000-000000006109'::uuid, '00000000-0000-7000-8000-000000002004'::uuid, 'usr_anong',  '00000000-0000-7000-8000-000000006108'::uuid,   'Perfect, that was the missing piece. Approving now.', now() - interval '20 hours')
    "#).execute(&mut *tx).await?;

    // ---- Review workflows + steps -----------------------------------------
    // One in-progress (contract-A12: legal approved, finance pending) and one
    // complete (budget-q1: both approved). Steps reference reviewer users so
    // list_workflow's JOIN renders real names.
    sqlx::query("DELETE FROM workflow_steps WHERE id::text LIKE '00000000-0000-7000-8000-000000007%'")
        .execute(&mut *tx).await?;
    sqlx::query("DELETE FROM file_workflows WHERE id::text LIKE '00000000-0000-7000-8000-000000007%'")
        .execute(&mut *tx).await?;
    sqlx::query(r#"
        INSERT INTO file_workflows (id, file_id, state, note, created_by, created_at) VALUES
          ('00000000-0000-7000-8000-000000007101'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 'Review',   'Routing for legal and finance sign-off before signature.', 'usr_anong', now() - interval '8 hours'),
          ('00000000-0000-7000-8000-000000007102'::uuid, '00000000-0000-7000-8000-000000002007'::uuid, 'Approved', 'Q1 budget final approval.',                                'usr_pat',   now() - interval '1 day')
    "#).execute(&mut *tx).await?;
    sqlx::query(r#"
        INSERT INTO workflow_steps (id, workflow_id, reviewer_id, decision, note, sequence, created_at, decided_at) VALUES
          ('00000000-0000-7000-8000-000000007201'::uuid, '00000000-0000-7000-8000-000000007101'::uuid, 'usr_wisanu', 'approved', 'Legal review passed — indemnification cap confirmed.', 1, now() - interval '8 hours', now() - interval '2 hours'),
          ('00000000-0000-7000-8000-000000007202'::uuid, '00000000-0000-7000-8000-000000007101'::uuid, 'usr_pat',    'pending',  NULL,                                                   2, now() - interval '8 hours', NULL),
          ('00000000-0000-7000-8000-000000007203'::uuid, '00000000-0000-7000-8000-000000007102'::uuid, 'usr_sarah',  'approved', 'Sales forecast aligned with the budget.',              1, now() - interval '1 day',   now() - interval '20 hours'),
          ('00000000-0000-7000-8000-000000007204'::uuid, '00000000-0000-7000-8000-000000007102'::uuid, 'usr_anong',  'approved', 'Approved for release.',                                2, now() - interval '1 day',   now() - interval '18 hours')
    "#).execute(&mut *tx).await?;

    // ---- Requests ----------------------------------------------------------
    // Form/document requests routed through the approval engine. These reuse the
    // two workflows seeded just above (contract-A12 in review, budget-q1
    // approved) as their anchor, so the Requests area is populated on a fresh
    // boot: one "waiting on you" for Pat, one approved in Anong's list.
    sqlx::query("DELETE FROM requests WHERE id::text LIKE '00000000-0000-7000-8000-00000000a1%'")
        .execute(&mut *tx).await?;
    sqlx::query(r#"
        INSERT INTO requests (id, kind, title, form_data, amount, file_id, system_id, org_id, ai_summary, created_by, created_at) VALUES
          ('00000000-0000-7000-8000-00000000a101'::uuid, 'document', 'Approve Phattana vendor contract (A12)',
            '{"document":"contract-A12.pdf","deadline":"10 Jul 2026","note":"Legal and finance sign-off before signature."}'::jsonb,
            NULL, '00000000-0000-7000-8000-000000002001'::uuid, 'sys_hr', 'org_phattana',
            'Vendor contract A12 for Phattana — legal has approved the indemnification cap; awaiting finance sign-off before signature.',
            'usr_anong', now() - interval '8 hours'),
          ('00000000-0000-7000-8000-00000000a102'::uuid, 'document', 'Approve Q1 2026 budget',
            '{"document":"budget-q1-2026.xlsx","note":"Q1 budget final approval before release."}'::jsonb,
            NULL, '00000000-0000-7000-8000-000000002007'::uuid, 'sys_fin', 'org_fin_ap',
            'Q1 budget — sales forecast aligned with the pipeline; both reviewers approved and it is ready for release.',
            'usr_pat', now() - interval '1 day')
    "#).execute(&mut *tx).await?;

    // ---- Notifications -----------------------------------------------------
    // For the two demo logins (admin + anong) so the bell shows a real unread
    // count and a mix of read/unread items.
    sqlx::query("DELETE FROM notifications WHERE id::text LIKE '00000000-0000-7000-8000-000000008%'")
        .execute(&mut *tx).await?;
    sqlx::query(r#"
        INSERT INTO notifications (id, user_id, kind, title, body, link, read_at, created_at) VALUES
          ('00000000-0000-7000-8000-000000008101'::uuid, 'usr_admin', 'review_requested',  'Review requested: contract-A12.pdf', 'Anong K. routed contract-A12.pdf for your review.',          '/requests/00000000-0000-7000-8000-00000000a101', NULL,                       now() - interval '3 hours'),
          ('00000000-0000-7000-8000-000000008102'::uuid, 'usr_admin', 'comment_mentioned', 'Pat S. mentioned you',               'Pat S. mentioned you in a comment on budget-q1-2026.xlsx.',   '/files/00000000-0000-7000-8000-000000002007', NULL,                       now() - interval '25 minutes'),
          ('00000000-0000-7000-8000-000000008103'::uuid, 'usr_admin', 'workflow_approved', 'Budget approved',                    'budget-q1-2026.xlsx completed its approval workflow.',       '/requests/00000000-0000-7000-8000-00000000a102', now() - interval '1 hour', now() - interval '1 hour'),
          ('00000000-0000-7000-8000-000000008104'::uuid, 'usr_anong', 'review_approved',   'Wisanu T. approved your file',       'Legal review passed on contract-A12.pdf.',                   '/requests/00000000-0000-7000-8000-00000000a101', NULL,                       now() - interval '2 hours')
    "#).execute(&mut *tx).await?;

    // ---- Version history ---------------------------------------------------
    // Historical rows point object_key at the file's current blob so every
    // "Download vN" still resolves (no orphaned-key 404s in the demo).
    sqlx::query("DELETE FROM file_versions WHERE id::text LIKE '00000000-0000-7000-8000-000000009%'")
        .execute(&mut *tx).await?;
    sqlx::query(r#"
        INSERT INTO file_versions (id, file_id, version, object_key, size_bytes, etag, uploaded_by, note, created_at) VALUES
          -- contract-A12.pdf (current v4)
          ('00000000-0000-7000-8000-000000009101'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 1, 'contracts/2026-Q1/contract-A12.pdf', 2410112, 'a1f2c84b91', 'Anong K.',  'Initial draft',                       now() - interval '6 days'),
          ('00000000-0000-7000-8000-000000009102'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 2, 'contracts/2026-Q1/contract-A12.pdf', 2456789, 'a1f2c84b92', 'Anong K.',  'Incorporated HR feedback',            now() - interval '4 days'),
          ('00000000-0000-7000-8000-000000009103'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 3, 'contracts/2026-Q1/contract-A12.pdf', 2498765, 'a1f2c84b93', 'Wisanu T.', 'Legal review edits to clause 4.2',    now() - interval '2 days'),
          ('00000000-0000-7000-8000-000000009104'::uuid, '00000000-0000-7000-8000-000000002001'::uuid, 4, 'contracts/2026-Q1/contract-A12.pdf', 2517299, 'a1f2c84b9e', 'Anong K.',  'Final redline — net-30 and cap fix',  now() - interval '28 minutes'),
          -- budget-q1-2026.xlsx (current v8) — recent slice of the history
          ('00000000-0000-7000-8000-000000009105'::uuid, '00000000-0000-7000-8000-000000002007'::uuid, 6, 'budgets/2026/budget-q1-2026.xlsx',   1120033, '02ee45ig96', 'Pat S.',    'Revised headcount plan',              now() - interval '3 days'),
          ('00000000-0000-7000-8000-000000009106'::uuid, '00000000-0000-7000-8000-000000002007'::uuid, 7, 'budgets/2026/budget-q1-2026.xlsx',   1142210, '02ee45ig97', 'Pat S.',    'Finance review edits',                now() - interval '1 day'),
          ('00000000-0000-7000-8000-000000009107'::uuid, '00000000-0000-7000-8000-000000002007'::uuid, 8, 'budgets/2026/budget-q1-2026.xlsx',   1153434, '02ee45ig99', 'Pat S.',    'Final sign-off',                      now() - interval '20 hours'),
          -- onboarding-2026.pdf (current v6)
          ('00000000-0000-7000-8000-000000009108'::uuid, '00000000-0000-7000-8000-000000002004'::uuid, 5, 'handbook/onboarding-2026.pdf',       2470000, 'd9bb12fd65', 'Krit M.',   'Draft refresh for 2026',              now() - interval '3 days'),
          ('00000000-0000-7000-8000-000000009109'::uuid, '00000000-0000-7000-8000-000000002004'::uuid, 6, 'handbook/onboarding-2026.pdf',       2517299, 'd9bb12fd66', 'Krit M.',   'Added benefits section + IT checklist', now() - interval '22 hours')
    "#).execute(&mut *tx).await?;

    // ---- Varied permissions ------------------------------------------------
    // 0002 only grants on contract-A12; spread realistic access across finance,
    // legal, and HR files so sharing looks actively managed. Own id group
    // (…0051xx) so we never touch the base grants (…0050xx).
    sqlx::query("DELETE FROM permissions WHERE id::text LIKE '00000000-0000-7000-8000-0000000051%'")
        .execute(&mut *tx).await?;
    sqlx::query(r#"
        INSERT INTO permissions (id, file_id, principal, principal_type, role, external) VALUES
          ('00000000-0000-7000-8000-000000005101'::uuid, '00000000-0000-7000-8000-000000002007'::uuid, 'Pat S. (pat@acme.go.th)',          'user',  'owner',  false),
          ('00000000-0000-7000-8000-000000005102'::uuid, '00000000-0000-7000-8000-000000002007'::uuid, 'Finance Admins',                   'group', 'manage', false),
          ('00000000-0000-7000-8000-000000005103'::uuid, '00000000-0000-7000-8000-000000002007'::uuid, 'Finance Viewers',                  'group', 'view',   false),
          ('00000000-0000-7000-8000-000000005104'::uuid, '00000000-0000-7000-8000-000000002006'::uuid, 'Wisanu T. (wisanu@acme.go.th)',    'user',  'owner',  false),
          ('00000000-0000-7000-8000-000000005105'::uuid, '00000000-0000-7000-8000-000000002006'::uuid, 'Legal team',                       'group', 'manage', false),
          ('00000000-0000-7000-8000-000000005106'::uuid, '00000000-0000-7000-8000-000000002006'::uuid, 'External Counsel (expires 30 Jun 2026)', 'link', 'view', true),
          ('00000000-0000-7000-8000-000000005107'::uuid, '00000000-0000-7000-8000-000000002004'::uuid, 'HR ส่วนกลาง',                       'group', 'manage', false),
          ('00000000-0000-7000-8000-000000005108'::uuid, '00000000-0000-7000-8000-000000002004'::uuid, 'All Employees',                    'group', 'view',   false)
    "#).execute(&mut *tx).await?;

    tx.commit().await?;
    tracing::info!("seeded demo collaboration data (comments, workflows, notifications, versions, permissions)");
    Ok(())
}
