# File Hub — roadmap

Planned and in-progress work, grouped by priority: **P0** (unblocks integration or
production), **P1** (correctness & UX at scale), **P2** (nice-to-have). Each item lists
**what / why / where / rough effort**. Contributions are welcome — see the README for how
to get a dev environment running, and [CLAUDE.md](CLAUDE.md) for architecture.

---

## P0 — unblocks integration & production

### 1. Shared login throttle for multi-pod deployments
- **What:** move the login throttle to a shared store (e.g. Redis) so it holds across
  replicas.
- **Why:** the throttle is in-process (`auth.rs::login_throttle`), so a multi-pod deploy
  effectively multiplies the allowed failure rate by the pod count.
- **Where:** `backend/src/auth.rs`.
- **Effort:** M.

### 2. Enforce (or remove) the remaining workspace access-policy toggles
- **What:** make every access-policy flag in Settings → General actually gate behaviour,
  or drop the ones that don't. (External-sharing and storage-quota flags are already
  honoured; audit the rest.)
- **Why:** a toggle that persists to `workspace_config` but is a silent no-op is worse
  than no toggle.
- **Where:** `backend/src/handlers.rs` (read `workspace_config` in the relevant guards) +
  `frontend/app/settings/general-form.tsx`.
- **Effort:** S–M per flag.

---

## P1 — correctness & UX at scale

### 3. Folder hierarchy navigation
- **What:** let users drill into folders in `/files` (breadcrumb + folder rows +
  `folder_id` filter), not just create them.
- **Why:** folders exist (`folders` table + `files.folder_id`) but the list is flat —
  there's no way to browse into a folder.
- **Where:** `frontend/app/files/*` + `GET /api/files?folder_id=` (the param exists on the
  upload path; ensure list honours it).
- **Effort:** M.

### 4. Bulk move + bulk share; inline rename
- **What:** extend the multi-select bulk bar (Download/Delete done) with Move-to-folder and
  Share; add inline rename from the row ⋯ menu.
- **Where:** `frontend/app/files/files-table.tsx` (`RowMenu` + bulk bar);
  `PATCH /api/files/{id}` already supports `name` / `folder_id`.
- **Effort:** M.

### 5. Serve the OpenAPI spec + Swagger UI; keep it in sync
- **What:** serve `backend/openapi.yaml` at `/fh/api/openapi.json` plus a `/fh/docs`
  Swagger UI; consider generating it from code (`utoipa`) so it can't drift.
- **Why:** the spec is hand-written today — easy to fall out of sync as routes change.
- **Where:** `backend/src/lib.rs` (static route) or `utoipa` derive on handlers.
- **Effort:** S (serve) / L (codegen migration).

### 6. Frontend test runner + smoke tests
- **What:** add Playwright and codify the core flows (login → filter → layout-switch
  preserves context → upload → appears; viewer RBAC hides controls).
- **Why:** there is no frontend test runner configured; all frontend verification is
  manual right now.
- **Where:** `frontend/` (Playwright config + `e2e/`).
- **Effort:** M.

### 7. CI pipeline
- **What:** on every PR, run `cargo test` (needs a Postgres service) + `cargo build
  --release` + `next build`, and gate merges on them.
- **Where:** `.github/workflows/`.
- **Effort:** S–M.

---

## P2 — nice-to-have

### 8. Board grouping by fields other than status
Board is hardcoded to status columns; allow group-by owner/project (turns it into a
general kanban). `frontend/app/files/board/page.tsx`. Effort: M.

### 9. Upload UX: drag-drop folders, retry failed, progress for TUS resume
`frontend/app/upload/page.tsx`. Effort: S–M.

### 10. Observability + upload rate limiting
Structured request logging, basic metrics (Prometheus), tracing spans around the upload
pipeline, and a rate limit on `POST /api/files` / TUS to blunt abuse. `backend/`. Effort: M.

### 11. Accessibility + mobile pass
Keyboard nav + `aria` on the popover menus (Filter/Sort/Group/Properties/row ⋯) and the
table; responsive layout check for the file views. `frontend/`. Effort: M.

---

## Shipped

- **API-key / `Bearer` auth** for machine-to-machine integrations (`auth.rs`,
  `migrations/0015_api_keys.sql`, documented in `INTEGRATION.md` + `openapi.yaml`).
- **Server-side sort + `owner` filter + total count** on `GET /api/files`
  (`handlers.rs::list_files`, `X-Total-Count` header).
- **Workspace access-policy enforcement** — external-sharing and storage-quota flags
  honoured by the backend guards.
- **MinIO / S3-compatible storage** — turnkey via `docker-compose.yml` + documented
  `S3_*` config; verified with a live round-trip.
- **Resumable upload (TUS 1.0.0)**, **WOPI** Office editing, **soft-delete + rotation
  worker**, and **AES-256-GCM at-rest encryption** (fs / s3).
- Viewer RBAC control-hiding · productionized login · Filter/Sort/Group/Properties +
  row ⋯ menu + multi-select bulk actions · dashboard activity tabs · cross-layout
  filter/search/sort preservation · `openapi.yaml` + `INTEGRATION.md`.
