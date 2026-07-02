# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Stack & layout

- **Backend** — Rust + Axum (`backend/`). Postgres via `sqlx` (not SQLite; **pgvector** for AI embeddings). Pluggable object storage (`fs` default, `s3`/MinIO available) with optional AES-256-GCM encryption layered in front. Optional **AI-native layer** (semantic search + RAG, local-first via Ollama) — see `ai.rs`/`ai_api.rs`/`ai_worker.rs`, toggle with `AI_ENABLED`.
- **Frontend** — Next.js 15 App Router + TypeScript (`frontend/`). React 18. No CSS framework — design tokens in `app/tokens.css`. Bilingual (EN/TH) via `lib/i18n.tsx`.
- **Infra** — `docker-compose.yml` runs Postgres (port 5434 → 5432), an *optional* MinIO (S3 API 9000, console 9001) + `minio-setup` bucket bootstrapper for the `s3` storage backend, and an *optional* branded Collabora Online container (port 9980) for in-browser Office editing. The Collabora image is built from `infra/collabora/` (Dockerfile patches `bundle.js` to empty the Help tab; `branding.css` rebrands the UI). `infra/litellm/config.yaml` is an *optional* OpenAI-compatible gateway for the AI layer (point `AI_BASE_URL` at it to centralise keys/routing across providers). The Postgres image is `pgvector/pgvector:pg16` (the AI migration needs `CREATE EXTENSION vector`).
- **Scripts** — `scripts/test-api.sh` (curl + python3 end-to-end suite), `seed-bodies.sh` (upload demo files), `backup.sh`/`restore.sh`.
- **CI** — `.github/workflows/ci.yml`: two parallel jobs on every push/PR — backend (cargo build + compile tests, with a Postgres service) and frontend (typecheck + production build).
- **Roadmap** — `TODO.md` tracks planned work (P0/P1/P2 with what/why/where per item). Check it before starting a feature — it may already be specced there.

## Running

```bash
# 1. Postgres (port 5434, user/pass/db = filehub)
docker compose up -d postgres

# 2. Backend on :8090 — runs migrations + seeds users on first start
cd backend && cargo run

# 3. Frontend — package.json says -p 3000, but .env / CORS_ORIGIN / redirects expect 3001.
#    Run with PORT=3001 or `next dev -p 3001` to match the rest of the system.
cd frontend && npm install && PORT=3001 npm run dev
```

Open <http://localhost:3001/filehub>. Seed login: `anong@acme.go.th` / `anong123` (editor), `admin@acme.go.th` / `admin123` (admin), `viewer@acme.go.th` / `viewer123` (viewer) — defined in `backend/src/auth.rs::SEED_ACCOUNTS`, inserted only when `users` is empty.

## Testing

```bash
# Backend integration tests hit a LIVE backend (no in-process server).
# Start `cargo run` first, then in another shell:
cd backend && cargo test                           # all tests
cd backend && cargo test health_returns_ok         # one test
FILEHUB_TEST_BASE=http://staging:8090 cargo test   # point at a different env

# End-to-end curl suite (also needs a running backend):
./scripts/test-api.sh
BASE=http://localhost:8090 ./scripts/test-api.sh
```

Tests log in as the seed `anong@acme.go.th` editor via `tests/common/mod.rs::auth_client()` and rely on the seed UUIDs in `migrations/0002_seed.sql` (e.g. `FILE_001` = contract-A12.pdf). If you add new fixtures, mirror the constants in `tests/common/mod.rs`. The TUS tests (`tus_*` in `tests/api.rs`) drive the full create → PATCH chunk → finalise path against the live backend and lock in three bugs (relative `Location` header, >2 MiB chunk body limit, empty-string metadata FK trip) — keep them green when touching `tus.rs`.

Liveness vs readiness: `/api/health` is a cheap static `"ok"`; `/api/ready` does a real DB + storage round-trip (use it for k8s `readinessProbe`).

Frontend: `npm run lint` and `npm run build`. Playwright e2e smoke tests live in `frontend/e2e/` (see its README):

```bash
# Needs the full live stack running (Postgres + backend :8090 + frontend :3001).
cd frontend
npx playwright install chromium       # one-time browser download
npm run e2e                           # all specs (headless)
npm run e2e -- search.spec.ts         # one spec
E2E_BASE_URL=https://staging.example.com/filehub npm run e2e   # other deploy
```

The e2e suite runs **serially** (`workers: 1` in `playwright.config.ts`) on purpose — the backend throttles logins per-process and the specs share the three seed accounts. Don't parallelize it.

## URL routing — the critical detail

The backend mounts **everything under `/fh`** (see `lib.rs::build_router` final line: `Router::new().nest("/fh", inner)`). The frontend runs under **basePath `/filehub`** (see `next.config.ts`). The browser talks to Next.js, and Next.js rewrites `/filehub/api/*` → `${BACKEND}/fh/api/*` so cookies are same-origin.

Consequences:
- Server-side fetches from Next.js code go directly to `http://127.0.0.1:8090/fh/...` (see `frontend/lib/api.ts` `BASE` and `next.config.ts` `BACKEND`). **Always `127.0.0.1`, never `localhost`** — backend binds `0.0.0.0` (v4 only) and macOS resolves `localhost` to `::1`, producing `ECONNREFUSED ::1:8090`.
- `next.config.ts` also adds defensive redirects for bare paths (`/files` → `/filehub/files`) because `basePath` only auto-prefixes `<Link>`, not `<a>`.
- Cookie name is `filehub_session`. `frontend/middleware.ts` is a UX gate that redirects unauthenticated traffic to `/login` but never validates the cookie — the backend re-validates every request against the `sessions` table.

## Backend architecture

- `main.rs` — env, tracing, spawns the rotation worker (`ROTATION_INTERVAL_SECS`, 3600s default, `0` disables), serves the router with **graceful shutdown** (SIGINT/SIGTERM drain so a k8s rollout doesn't abort a mid-flight upload or rotation tick).
- `lib.rs::build_router` — single source of truth for routes. **Authentication is enforced by a tower layer over the `private` sub-router** (`require_session`), not by per-handler extractors — adding a route inside `private` can't forget the session check. Multipart routes (file CRUD + TUS) sit in nested sub-routers carrying `DefaultBodyLimit::max(64 MiB)`; Axum's 2 MiB default would otherwise silently reject large uploads. `CORS_ORIGIN`/`WOPI_SECRET` are read here and **panic in release if unset**. API docs are public routes: Swagger UI at `/fh/docs`, build-time-embedded spec at `/fh/api/openapi.yaml` — update the spec when adding/changing endpoints.
- `state.rs` — `AppState { db: PgPool, storage: Storage }`. Tunable pool (`DATABASE_MAX_CONNECTIONS` etc.), then `sqlx::migrate!` → `bootstrap_seed_users` → `seed_demo::bootstrap_demo_data`. Release builds fail-fast on missing required env (`check_required_env_release`).
- `seed_demo.rs` — demo collaboration rows (activity, comments, workflows, notifications, versions) that make a fresh install look lived-in. Lives in Rust, **not** a migration, because `sqlx::migrate!` checksums forbid editing `0002_seed.sql` and these rows FK to users that only exist after `bootstrap_seed_users`. Idempotent: fixed recognizable UUIDs are DELETEd + re-INSERTed with `now()`-relative timestamps on every boot, so the demo stays fresh without touching runtime rows (which use UUIDv7). Best-effort — a failure is logged and swallowed, never blocks startup. Demo runs should set `ROTATION_INTERVAL_SECS=0` so rotation doesn't prune the seeded versions/trash.
- `auth.rs` — argon2id hashing, 32-byte random session tokens in `sessions` (14-day TTL), `AuthUser`/`MaybeAuthUser` extractors. Cookie `Secure` flag via `COOKIE_SECURE`. **Login is throttled** by an in-process `login_throttle` module (10 fails/min → 60s lockout) with a constant-time dummy-hash path on unknown emails to kill timing oracles — note it's per-process, so a multi-pod deploy needs a shared store.

### Authorization model (authn ≠ authz — both layers matter)

`require_session` only proves *a* valid session exists. **Per-resource authorization is the handler's job** via three `auth.rs` helpers — a new handler that skips them reintroduces the IDOR this was built to close:
- `require_role(&user.0, &["admin", "editor"])?` — gate every mutating handler (upload, patch, delete, folder/share/comment CUD). Viewers are read-only.
- `ensure_system_access(&db, &user, system_id).await?` — before touching any caller-supplied file/system id. Returns 403 if a non-admin reaches into someone else's `system_type='personal'` drive; **returns Ok for a non-existent system** so the handler's own "unknown system" path (400/empty) still wins. Reads map the denial to 404 (don't leak existence); writes let 403 propagate.
- `effective_system_ids(&db, &user).await?` — `None` for admins (see everything) or `Some(vec)` to scope list/search queries with `AND system_id = ANY($n)`.
- `handlers.rs` — the bulk of CRUD: stats, systems, orgs, files (incl. multipart upload, download, soft-delete), folders, share links, workspace config, members, rotation, search, reports, activity, views.
- `p1.rs` — workflow / comments / notifications / thumbnails / PDF text extraction / preview pipeline. Co-located so the four features can share helpers. Office docs (docx/xlsx/pptx/odf) are full-text extracted by routing through the same LibreOffice→PDF conversion the preview uses.
- `ai.rs` + `ai_api.rs` + `ai_worker.rs` — **AI-native layer, local-first and pluggable.** `ai.rs` is one HTTP client against the OpenAI-compatible API shape (Ollama by default — zero egress; any OpenAI-compatible endpoint via `AI_BASE_URL`/`AI_API_KEY`/model names, no code change). `ai_worker.rs` drains the `ai_jobs` queue (extract → chunk → embed → summarise), enqueued from both upload paths (`handlers::persist_upload`, `tus::finalise`); spawned only when `Ai::enabled()`. `ai_api.rs` serves the read side: semantic search (pgvector kNN), `/api/ask` RAG (grounded + cited), and per-file AI. **All retrieval is scoped by `effective_system_ids`** — a user can never get an answer drawn from content they can't read. Set `AI_ENABLED=false` to disable everything; FileHub then behaves exactly as before. Embedding dim is fixed at 768 (`migrations/0016`); changing models to another dim needs a migration + re-embed.
- `checkout.rs` — document check-out / check-in locking (TOR 5.3.8.4-5). Lock state is three nullable columns on `files` (`migration 0017`); enforcement on edits lives in the mutating handlers via `checkout::lock_blocks` (wired into `handlers::upload_version`). `rotation.rs` auto-releases stale locks (`CHECKOUT_TTL_HOURS`, default 8h).
- `reports.rs` — document status report (active/inactive/retention/deleted, mapping FileHub's soft-delete + rotation lifecycle onto the four TOR states) + CSV exports (status, audit log) with a UTF-8 BOM so Excel renders Thai. Also the admin-only AI-usage metering report. Permission-scoped via `effective_system_ids`.
- `stars.rs` — per-user file favorites ("Starred" in the everyday UI). `file_stars` (migration 0018); `GET /api/starred` (list) + `GET/PUT/DELETE /api/files/:id/star`. No access → 404, like reads.
- `tus.rs` — TUS 1.0.0 resumable upload (`creation` + `termination` extensions). On final chunk, runs the same encrypt → ObjectStore → DB → preview pipeline as a regular upload.
- `wopi.rs` — WOPI host for Collabora/OnlyOffice. Per-file access tokens are HMAC-SHA256 (no DB lookup on every request).
- `rotation.rs` — background worker for version pruning, archive (soft delete), hard delete. Resolution rule: **most-specific wins (user > org > system > workspace)**. Hard-delete is transactional and re-checks the `deleted_at` cutoff inside the `DELETE` (so a concurrent restore wins the race) and preserves `activity` rows (FK is `ON DELETE SET NULL`). Each tick also reaps abandoned TUS sessions (`tus_uploads` past `expires_at` + their `.part` blobs).
- `storage.rs` + `store/{fs,s3}.rs` — `Storage` is a wrapper that optionally AES-256-GCM-encrypts before delegating to whichever `ObjectStore` impl is selected by `STORAGE_BACKEND` (`fs` or `s3`). The ETag is MD5 of the **plaintext** so re-keying doesn't change it. `STORAGE_ENC_KEY` is a hex-encoded 32-byte key. `fs::put` is **write-tmp-then-atomic-rename + fsync** so a crash mid-write can't leave a torn blob. User-supplied filenames pass through `handlers::sanitize_filename` before becoming object keys (the original name is kept in the DB for display).

### ID strategy (hybrid — see `migrations/0001_init.sql` header)

- **System tables** (`systems`, `orgs`, `folders`, `views`, `users`) → `TEXT` PKs, app-generated CUID2. Seed rows use readable literals (`sys_hr`, `org_phattana`) so URLs and test asserts stay debuggable.
- **Transaction tables** (`files`, `activity`, `sessions`, `share_links`, `comments`, `notifications`, `workflow*`, `file_versions`, `thumbnails`) → `UUID` PKs, UUIDv7 for insert-order index locality.

FK column types follow the referenced PK type. Models in `models.rs` reflect this: `System.id: String`, `File.id: Uuid`.

## Frontend architecture

### Two personas — Workspace vs Everyday (one codebase, a view-mode switch)

The frontend serves **two shells over the same data + design system**:
- **Workspace** (power UI, admins) — the sidebar app: `/` dashboard, systems/orgs tree, reports, audit, members, technical file detail. This is the "original" app.
- **Everyday** (consumer UI, viewers/editors) — the `app/(everyday)/` route group (URL-transparent): a clean top-bar shell over `/home`, `/my`, `/shared`, `/recent`, `/starred`, and a friendly `/f/[id]` file view. Plain language (Systems → **"Areas"**), preview-first, minimal columns, no ETag/quota/rotation vocabulary. On phones the top nav becomes a bottom tab bar; `MobileUploadFab` hides on everyday routes (the bottom nav has its own Upload).

`lib/view-mode.tsx` holds the mode (`fh-view` cookie, read server-side in `app/layout.tsx`). **Default by role once auth resolves: admin → workspace, everyone else → everyday.** `app/page.tsx` (the `/` dashboard) redirects everyday-mode users to `/home`; the user menu in each shell has the reverse switch. Everyday components live in `components/everyday/*` and reuse the workspace `FilePreview`/`FileAiPanel`. Keep the two shells' chrome distinct but the data layer shared.

- `app/` — App Router pages. Workspace routes map 1:1 to the README screens (`/`, `/files` + layout variants `board`/`gallery`/`calendar`/`timeline`, `/files/[id]`, `/upload`, `/views`, `/views/new`, `/orgs`, `/share`, `/settings` + sub-pages `members`/`roles`/`workflows`/`audit`, `/login`, `/trash`, `/activity`, `/archive`); the everyday routes live under `app/(everyday)/`.
- `components/` — `sidebar.tsx`, `topbar.tsx`, `primitives.tsx` (shared UI), `icons.tsx` (40+ stroke icons inline), `notifications-bell.tsx`, `global-search.tsx`, `user-menu.tsx`, `pager.tsx` (shared pagination for the list pages), `workflow-panel.tsx` (shared approval-workflow panel — start from ad-hoc reviewers or a saved template, turn-guarded approve/reject, send-back; used by both the workspace `app/files/[id]/sidecar.tsx` and the everyday file view; templates are managed at `/settings/workflows`).
- `lib/roles.ts` — **frontend mirror of the backend role gate.** `canMutate(role)` (admin+editor) / `isAdmin(role)` decide whether to *render* mutating controls so viewers aren't handed buttons that bounce with a 403; the backend `require_role` remains the real enforcement. Null/loading roles fail closed. Use these helpers instead of comparing role strings inline.
- `lib/sidebar-context.tsx` — responsive sidebar/drawer state (the sidebar collapses to a drawer on narrow viewports via `tokens.css` breakpoints).
- `lib/api.ts` — typed fetch wrappers + DTO types. Server-side uses `BACKEND_URL` (`http://127.0.0.1:8090`) directly; client-side hits the basePath-prefixed proxy. **Every `safe*` helper takes a trailing optional `cookieHeader` arg.**
- `lib/auth-server.ts::loadServerCtx()` — **the cookie-forwarding contract for server components.** Pages that render private data MUST `const { cookieHeader, role } = await loadServerCtx()` and thread `cookieHeader` into each `safe*` call. Forget it and the server-side `fetch` hits the backend anonymously → 401 → the `safe*` helper swallows it and the page renders empty stats / an empty sidebar (no error). `app/settings/_shared.ts` re-exports this as `loadSettingsCtx` for back-compat.
- `lib/auth-context.tsx` — *client*-side React context wrapping `/api/auth/me`; the browser sends the cookie naturally so no forwarding needed here.
- `middleware.ts` — basePath-stripped paths (Next.js strips `/filehub` before middleware sees them). Allowlist is `/login`, `/api/*`, `/_next/*`, `/branding/*`, `favicon.ico`.

## Environment variables

Backend (`backend/.env`):
- `DATABASE_URL` — **required in release**, default `postgresql://filehub:filehub@localhost:5434/filehub` in debug.
- `PORT` — default `8090`.
- `STORAGE_BACKEND` — `fs` (default) or `s3`. The `s3` backend uses hand-rolled SigV4 (no `aws-sdk-s3`) and is S3-compatible — works with AWS S3, **MinIO**, Wasabi, R2. Configure via `S3_ENDPOINT` / `S3_REGION` / `S3_BUCKET` / `S3_ACCESS_KEY` / `S3_SECRET_KEY` / `S3_PATH_STYLE`. `docker-compose.yml` ships a `minio` + `minio-setup` service (creates the `filehub-storage` bucket); `docker compose up -d minio minio-setup` plus the `S3_*` block in `backend/.env.example` is a turnkey local setup. **MinIO needs path-style addressing — keep `S3_PATH_STYLE=true`** (the backend default). Encryption (`STORAGE_ENC_KEY`) layers in front of any backend unchanged.
- `STORAGE_ROOT` — fs backend root, default `./storage`.
- `STORAGE_ENC_KEY` — optional hex-encoded 32 bytes; when set, all writes are AES-256-GCM encrypted. **Backups must capture this** — see `scripts/backup.sh`.
- `CORS_ORIGIN` — **required in release**, single allowed origin (must be exact, not `*`, because cookies require credentials). Debug fallback `http://localhost:3001`.
- `COOKIE_SECURE` — `true` (default) to mark session cookies `Secure`. Set `false` only for local http:// dev.
- `WOPI_SECRET` — **required in release**, HMAC key for Collabora access tokens. Generate with `openssl rand -hex 32`.
- `DATABASE_MAX_CONNECTIONS` / `DATABASE_ACQUIRE_TIMEOUT_SECS` / `DATABASE_IDLE_TIMEOUT_SECS` — pool tuning. Defaults `32` / `30` / `600`.
- `ROTATION_INTERVAL_SECS` — rotation worker cadence; `0` disables.
- `CHECKOUT_TTL_HOURS` — auto-release a check-out lock held longer than this (reaped by the rotation worker). Default `8`.
- `AI_ENABLED` — master switch for the AI-native layer (default `true`). `false` skips the enrichment worker and makes `/api/search/semantic` + `/api/ask` return 400 — FileHub behaves exactly as before AI.
- `AI_BASE_URL` / `AI_API_KEY` — OpenAI-compatible endpoint + optional key. Default `http://localhost:11434/v1` (Ollama, no key). Point at vLLM / LiteLLM / OpenAI / Azure to switch providers with no code change.
- `AI_EMBED_MODEL` / `AI_EMBED_DIM` — default `nomic-embed-text` / `768`. **`AI_EMBED_DIM` must match the `vector(768)` column in `migrations/0016`** (the backend warns at boot on mismatch); a different dim needs a migration + re-embed.
- `AI_CHAT_MODEL` / `AI_MAX_CONTEXT_TOKENS` / `AI_TIMEOUT_SECS` — chat model (default `qwen2.5`) and limits.
- `AI_WORKER_INTERVAL_SECS` — enrichment queue poll cadence. Default `15`.
- `RUST_LOG` — tracing filter.

> **AI requires pgvector.** `docker-compose.yml` uses `pgvector/pgvector:pg16`; `migrations/0016` runs `CREATE EXTENSION vector`. Local AI quick start: `ollama serve` then `ollama pull nomic-embed-text qwen2.5`.

Frontend (`frontend/.env`):
- `BACKEND_URL` — used by both server-side `lib/api.ts` and the `next.config.ts` rewrite. Default `http://127.0.0.1:8090`.

## Production checklist

Release builds (`cargo build --release`) fail-fast if `DATABASE_URL`, `CORS_ORIGIN`, or `WOPI_SECRET` is missing. Before exposing the stack to real users:

1. **Rotate secrets** — generate fresh `STORAGE_ENC_KEY` (`openssl rand -hex 32`) + `WOPI_SECRET` per environment. Never reuse the dev values.
2. **Change seed credentials** — `backend/src/auth.rs::SEED_ACCOUNTS` ships `admin123`/`anong123`/`viewer123`. They only seed when `users` is empty, but if you ever bootstrap a prod DB you must change them immediately (or delete + recreate via the admin UI).
3. **Verify cookie `Secure`** — `COOKIE_SECURE=true` (default) requires HTTPS. Browser will refuse to send the session cookie over plain http://.
4. **Postgres credentials** — `docker-compose.yml` ships `filehub:filehub`. Replace with env-injected secrets before prod use.
5. **Backups** — run `scripts/backup.sh` daily; the script refuses to run without `STORAGE_ENC_KEY` (set `ALLOW_MISSING_ENC_KEY=1` only if encryption is intentionally off). Test `scripts/restore.sh` on a separate host before relying on it.
6. **Migrations** — `cargo run` runs `sqlx::migrate!` on boot. For zero-downtime prod deploys, run migrations as a one-shot job first, then roll new replicas.
7. **Collabora behind TLS** — `docker-compose.yml` runs Collabora with SSL disabled for dev. In prod terminate TLS at a reverse proxy and set `--o:ssl.termination=true`. Hardcoded `host.docker.internal:8090` references must be replaced.
8. **Quota + rotation** — set workspace/system/org/user quotas via the admin UI before unblocking uploads, and confirm `ROTATION_INTERVAL_SECS` is non-zero so trash gets purged.

## Conventions worth knowing

- Workspace identity (name, display name) and the storage quota live in the `workspace_config` k/v table, not in a config file. Read in `handlers::stats`, written via `PATCH /api/workspace`.
- Personal "My Drive" systems (`system_type = 'personal'`) are filtered out of the workspace dashboard (`storage_by_system`, `connected_systems`) and can't be renamed via `PATCH /api/systems/:id`.
- Soft delete: `files.deleted_at IS NOT NULL` means trashed; `rotation.rs` is what eventually purges the bytes.
- Activity rows are created by handlers as a side effect of mutating endpoints (search for `INSERT INTO activity`). New mutating handlers should follow suit so the activity feed stays accurate.
- Share links carry a one-shot token in the URL — that token IS the credential, so `share_meta` / `share_download` live in the *public* router.
- List sorting/owner filtering is **frontend-side** — `GET /api/files` has no sort/owner params; pages fetch and sort/filter client-side, and pagination on the list pages (Trash/Activity/Audit/Archive/Members) is `components/pager.tsx` over the already-fetched rows. Adding a backend param means also removing the frontend equivalent.
- The upload page picks transport by size: small files go as one multipart POST, large files use `tus-js-client` resumable upload (8 MB PATCH chunks) against `tus.rs` — see the threshold constant in `app/upload/page.tsx`.
- `tokens.css` contains a full `.dark` palette but nothing toggles it yet — dark mode is scaffolded, not shipped.
