# File Hub — development backlog

Prioritized follow-ups gathered after the UI-productionization + flow + Chrome-MCP review
pass. Each item: **what / why / where / rough effort**. Tiers are P0 (unblocks
integration or prod), P1 (correctness + UX at scale), P2 (nice-to-have).

---

## P0 — unblocks integration & production

### 1. API key / Bearer auth for machine-to-machine
- **What:** an `api_keys` table + an `Authorization: Bearer <key>` extractor alongside the
  session-cookie path; scope keys to a service account + role.
- **Why:** today auth is session-cookie only (14-day). Any external app that wants to
  **upload** must log in with a real account and refresh the session — there is no headless
  credential. This is the #1 blocker for "other apps coming to upload."
- **Where:** `backend/src/auth.rs` (new extractor + table), `migrations/`, document in
  `backend/openapi.yaml` (`securitySchemes`) + `INTEGRATION.md`.
- **Effort:** M (1–2 days).

### 2. Server-side sort + `owner` filter + real pagination on `GET /api/files`
- **What:** accept `sort`/`dir`/`owner` params; return a total count (header or envelope);
  page via `limit`/`offset` server-side.
- **Why:** `list_files` hardcodes `ORDER BY modified_at DESC` and ignores `owner`, so the
  frontend fetches up to `LIST_LIMIT=1000` and sorts/filters/paginates client-side. Breaks
  past 1000 files; sort/filter aren't whole-dataset correct.
- **Where:** `backend/src/handlers.rs::list_files` (+ `FilesQuery`); then simplify
  `frontend/app/files/page.tsx` (drop the client-side sort/slice once the server does it).
- **Effort:** M.

### 3. Enforce (or remove) the workspace access-policy toggles
- **What:** make the access-policy flags in Settings → General actually gate behaviour, or
  drop the ones that don't.
- **Why:** `frontend/app/settings/general-form.tsx` admits some toggles persist to
  `workspace_config` but "are not yet honoured by the backend" — a silent no-op is worse
  than no toggle.
- **Where:** `backend/src/handlers.rs` (read `workspace_config` in the relevant guards) +
  the form.
- **Effort:** S–M (per flag).

### 4. Production hardening (pre-launch checklist)
- **What:** rotate `STORAGE_ENC_KEY` + `WOPI_SECRET` per env; change seed creds; move the
  per-process login throttle to a shared store (Redis) for multi-pod; verify `COOKIE_SECURE`
  + TLS; env-inject Postgres creds.
- **Why:** the throttle is in-process (`auth.rs::login_throttle`) so it doesn't hold across
  replicas; seed creds (`admin123` …) ship in `auth.rs::SEED_ACCOUNTS`.
- **Where:** see `CLAUDE.md` → "Production checklist"; `backend/src/auth.rs`.
- **Effort:** S (config) + M (shared throttle).

---

## P1 — correctness & UX at scale

### 5. Folder hierarchy navigation
- **What:** let users drill into folders in `/files` (breadcrumb + folder rows + `folder_id`
  filter), not just create them.
- **Why:** folders exist (`NewFolderButton` + `folders` table + `files.folder_id`) but the
  list is flat — there's no way to browse into a folder.
- **Where:** `frontend/app/files/*` + `GET /api/files?folder_id=` (param exists in the
  upload path; ensure list honours it).
- **Effort:** M.

### 6. Bulk move + bulk share; inline rename
- **What:** extend the multi-select bulk bar (Download/Delete done) with Move-to-folder and
  Share; add inline rename from the row ⋯ menu.
- **Where:** `frontend/app/files/files-table.tsx` (`RowMenu` + bulk bar);
  `PATCH /api/files/{id}` already supports `name`/`folder_id`.
- **Effort:** M.

### 7. Serve the OpenAPI spec + Swagger UI; keep it in sync
- **What:** serve `backend/openapi.yaml` at `/fh/api/openapi.json` + a `/fh/docs` Swagger UI;
  consider generating it from code (`utoipa`) so it can't drift.
- **Why:** the spec is hand-written today — easy to fall out of sync as routes change.
- **Where:** `backend/src/lib.rs` (static route) or `utoipa` derive on handlers.
- **Effort:** S (serve) / L (codegen migration).

### 8. Frontend test runner + smoke tests
- **What:** add Playwright; codify the flows verified manually this session (login →
  filter → layout-switch preserves context → upload → appears; viewer RBAC hides controls).
- **Why:** `CLAUDE.md`: "There is no frontend test runner configured." All FE verification
  is manual right now.
- **Where:** `frontend/` (Playwright config + `e2e/`).
- **Effort:** M.

### 9. CI pipeline
- **What:** on PR run `cargo test` (needs a Postgres service) + `cargo build --release` +
  `next build`; gate merges.
- **Where:** `.github/workflows/`.
- **Effort:** S–M.

---

## P2 — nice-to-have

### 10. Board grouping by fields other than status
- Board is hardcoded to status columns; allow group-by owner/project (turns it into a
  general kanban). `frontend/app/files/board/page.tsx`. Effort: M.

### 11. Upload UX: drag-drop folders, retry failed, progress for TUS resume
- `frontend/app/upload/page.tsx`. Effort: S–M.

### 12. Observability + upload rate limiting
- Structured request logging, basic metrics (Prometheus), tracing spans around the upload
  pipeline; a rate limit on `POST /api/files` / TUS to blunt abuse. `backend/`. Effort: M.

### 13. Accessibility + mobile pass
- Keyboard nav + `aria` on the new popover menus (Filter/Sort/Group/Properties/row ⋯) and
  the table; responsive layout check for the file views. `frontend/`. Effort: M.

---

### Already shipped this cycle (for reference)
Viewer RBAC control-hiding · productionized login (demo panel behind a flag) · wired
Filter/Sort/Group/Properties + row ⋯ menu + multi-select bulk · dashboard activity tabs ·
cross-layout filter/search/sort preservation · client-side pagination · post-upload CTA ·
`backend/openapi.yaml` + `backend/INTEGRATION.md`.
