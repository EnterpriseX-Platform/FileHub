# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Stack & layout

- **Backend** — Rust + Axum (`backend/`). Postgres via `sqlx` (no SQLite — README is out of date). Pluggable object storage (`fs` default, `s3` available) with optional AES-256-GCM encryption layered in front.
- **Frontend** — Next.js 15 App Router + TypeScript (`frontend/`). React 18. No CSS framework — design tokens in `app/tokens.css`.
- **Infra** — `docker-compose.yml` runs Postgres (port 5434 → 5432) and an *optional* branded Collabora Online container (port 9980) for in-browser Office editing.
- **Scripts** — `scripts/test-api.sh` (curl + python3 end-to-end suite), `seed-bodies.sh` (upload demo files), `backup.sh`/`restore.sh`.

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

Frontend: `npm run lint` and `npm run build`. There is no frontend test runner configured.

## URL routing — the critical detail

The backend mounts **everything under `/fh`** (see `lib.rs::build_router` final line: `Router::new().nest("/fh", inner)`). The frontend runs under **basePath `/filehub`** (see `next.config.ts`). The browser talks to Next.js, and Next.js rewrites `/filehub/api/*` → `${BACKEND}/fh/api/*` so cookies are same-origin.

Consequences:
- Server-side fetches from Next.js code go directly to `http://127.0.0.1:8090/fh/...` (see `frontend/lib/api.ts` `BASE` and `next.config.ts` `BACKEND`). **Always `127.0.0.1`, never `localhost`** — backend binds `0.0.0.0` (v4 only) and macOS resolves `localhost` to `::1`, producing `ECONNREFUSED ::1:8090`.
- `next.config.ts` also adds defensive redirects for bare paths (`/files` → `/filehub/files`) because `basePath` only auto-prefixes `<Link>`, not `<a>`.
- Cookie name is `filehub_session`. `frontend/middleware.ts` is a UX gate that redirects unauthenticated traffic to `/login` but never validates the cookie — the backend re-validates every request against the `sessions` table.

## Backend architecture

- `main.rs` — env, tracing, spawns the rotation worker (`ROTATION_INTERVAL_SECS`, 3600s default, `0` disables), serves the router with **graceful shutdown** (SIGINT/SIGTERM drain so a k8s rollout doesn't abort a mid-flight upload or rotation tick).
- `lib.rs::build_router` — single source of truth for routes. **Authentication is enforced by a tower layer over the `private` sub-router** (`require_session`), not by per-handler extractors — adding a route inside `private` can't forget the session check. Multipart routes (file CRUD + TUS) sit in nested sub-routers carrying `DefaultBodyLimit::max(64 MiB)`; Axum's 2 MiB default would otherwise silently reject large uploads. `CORS_ORIGIN`/`WOPI_SECRET` are read here and **panic in release if unset**.
- `state.rs` — `AppState { db: PgPool, storage: Storage }`. Tunable pool (`DATABASE_MAX_CONNECTIONS` etc.), then `sqlx::migrate!` → `bootstrap_seed_users`. Release builds fail-fast on missing required env (`check_required_env_release`).
- `auth.rs` — argon2id hashing, 32-byte random session tokens in `sessions` (14-day TTL), `AuthUser`/`MaybeAuthUser` extractors. Cookie `Secure` flag via `COOKIE_SECURE`. **Login is throttled** by an in-process `login_throttle` module (10 fails/min → 60s lockout) with a constant-time dummy-hash path on unknown emails to kill timing oracles — note it's per-process, so a multi-pod deploy needs a shared store.

### Authorization model (authn ≠ authz — both layers matter)

`require_session` only proves *a* valid session exists. **Per-resource authorization is the handler's job** via three `auth.rs` helpers — a new handler that skips them reintroduces the IDOR this was built to close:
- `require_role(&user.0, &["admin", "editor"])?` — gate every mutating handler (upload, patch, delete, folder/share/comment CUD). Viewers are read-only.
- `ensure_system_access(&db, &user, system_id).await?` — before touching any caller-supplied file/system id. Returns 403 if a non-admin reaches into someone else's `system_type='personal'` drive; **returns Ok for a non-existent system** so the handler's own "unknown system" path (400/empty) still wins. Reads map the denial to 404 (don't leak existence); writes let 403 propagate.
- `effective_system_ids(&db, &user).await?` — `None` for admins (see everything) or `Some(vec)` to scope list/search queries with `AND system_id = ANY($n)`.
- `handlers.rs` — the bulk of CRUD: stats, systems, orgs, files (incl. multipart upload, download, soft-delete), folders, share links, workspace config, members, rotation, search, reports, activity, views.
- `p1.rs` — workflow / comments / notifications / thumbnails / PDF text extraction / preview pipeline. Co-located so the four features can share helpers.
- `tus.rs` — TUS 1.0.0 resumable upload (`creation` + `termination` extensions). On final chunk, runs the same encrypt → ObjectStore → DB → preview pipeline as a regular upload.
- `wopi.rs` — WOPI host for Collabora/OnlyOffice. Per-file access tokens are HMAC-SHA256 (no DB lookup on every request).
- `rotation.rs` — background worker for version pruning, archive (soft delete), hard delete. Resolution rule: **most-specific wins (user > org > system > workspace)**. Hard-delete is transactional and re-checks the `deleted_at` cutoff inside the `DELETE` (so a concurrent restore wins the race) and preserves `activity` rows (FK is `ON DELETE SET NULL`). Each tick also reaps abandoned TUS sessions (`tus_uploads` past `expires_at` + their `.part` blobs).
- `storage.rs` + `store/{fs,s3}.rs` — `Storage` is a wrapper that optionally AES-256-GCM-encrypts before delegating to whichever `ObjectStore` impl is selected by `STORAGE_BACKEND` (`fs` or `s3`). The ETag is MD5 of the **plaintext** so re-keying doesn't change it. `STORAGE_ENC_KEY` is a hex-encoded 32-byte key. `fs::put` is **write-tmp-then-atomic-rename + fsync** so a crash mid-write can't leave a torn blob. User-supplied filenames pass through `handlers::sanitize_filename` before becoming object keys (the original name is kept in the DB for display).

### ID strategy (hybrid — see `migrations/0001_init.sql` header)

- **System tables** (`systems`, `orgs`, `folders`, `views`, `users`) → `TEXT` PKs, app-generated CUID2. Seed rows use readable literals (`sys_hr`, `org_phattana`) so URLs and test asserts stay debuggable.
- **Transaction tables** (`files`, `activity`, `sessions`, `share_links`, `comments`, `notifications`, `workflow*`, `file_versions`, `thumbnails`) → `UUID` PKs, UUIDv7 for insert-order index locality.

FK column types follow the referenced PK type. Models in `models.rs` reflect this: `System.id: String`, `File.id: Uuid`.

## Frontend architecture

- `app/` — App Router pages. Routes map 1:1 to the screens in the README (`/`, `/files`, `/files/board`, `/files/gallery`, `/files/[id]`, `/upload`, `/views/new`, `/orgs`, `/share`, `/settings`, `/login`, `/trash`, `/activity`, `/archive`).
- `components/` — `sidebar.tsx`, `topbar.tsx`, `primitives.tsx` (shared UI), `icons.tsx` (40+ stroke icons inline), `notifications-bell.tsx`, `global-search.tsx`, `user-menu.tsx`.
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
- `RUST_LOG` — tracing filter.

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

Frontend (`frontend/.env`):
- `BACKEND_URL` — used by both server-side `lib/api.ts` and the `next.config.ts` rewrite. Default `http://127.0.0.1:8090`.

## Conventions worth knowing

- Workspace identity (name, display name) and the storage quota live in the `workspace_config` k/v table, not in a config file. Read in `handlers::stats`, written via `PATCH /api/workspace`.
- Personal "My Drive" systems (`system_type = 'personal'`) are filtered out of the workspace dashboard (`storage_by_system`, `connected_systems`) and can't be renamed via `PATCH /api/systems/:id`.
- Soft delete: `files.deleted_at IS NOT NULL` means trashed; `rotation.rs` is what eventually purges the bytes.
- Activity rows are created by handlers as a side effect of mutating endpoints (search for `INSERT INTO activity`). New mutating handlers should follow suit so the activity feed stays accurate.
- Share links carry a one-shot token in the URL — that token IS the credential, so `share_meta` / `share_download` live in the *public* router.
