# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Stack & layout

- **Backend** — Rust + Axum (`backend/`). Postgres via `sqlx` (not SQLite). Pluggable object storage (`fs` default, `s3`/MinIO available) with optional AES-256-GCM encryption layered in front.
- **Frontend** — Next.js 15 App Router + TypeScript (`frontend/`). React 18. No CSS framework — design tokens in `app/tokens.css`.
- **Infra** — `docker-compose.yml` runs Postgres (port 5434 → 5432), an *optional* MinIO (S3 API 9000, console 9001) + `minio-setup` bucket bootstrapper for the `s3` storage backend, and an *optional* branded Collabora Online container (port 9980) for in-browser Office editing. The Collabora image is built from `infra/collabora/` (Dockerfile patches `bundle.js` to empty the Help tab; `branding.css` rebrands the UI).
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

Open <http://localhost:3001/filehub>. Seed login: `anong@acme.go.th` / `anong123` (editor), `admin@acme.go.th` / `admin123` (admin), `viewer@acme.go.th` / `viewer123` (viewer) — defined in `backend/src/auth.rs::SEED_ACCOUNTS`, inserted only with `DEMO_MODE=1` (tests need it: `DEMO_MODE=1 cargo run`).

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
- `p1.rs` — workflow / comments / notifications / thumbnails / PDF text extraction / preview pipeline. Co-located so the four features can share helpers.
- `tus.rs` — TUS 1.0.0 resumable upload (`creation` + `termination` extensions). On final chunk, runs the same encrypt → ObjectStore → DB → preview pipeline as a regular upload.
- `wopi.rs` — WOPI host for Collabora/OnlyOffice. Per-file access tokens are HMAC-SHA256 (no DB lookup on every request).
- `rotation.rs` — background worker for version pruning, archive (soft delete), hard delete. Resolution rule: **most-specific wins (user > org > system > workspace)**. Hard-delete is transactional and re-checks the `deleted_at` cutoff inside the `DELETE` (so a concurrent restore wins the race) and preserves `activity` rows (FK is `ON DELETE SET NULL`). Each tick also reaps abandoned TUS sessions (`tus_uploads` past `expires_at` + their `.part` blobs).
- `storage.rs` + `store/{fs,s3}.rs` — `Storage` is a wrapper that optionally AES-256-GCM-encrypts before delegating to whichever `ObjectStore` impl is selected by `STORAGE_BACKEND` (`fs` or `s3`). The ETag is MD5 of the **plaintext** so re-keying doesn't change it. `STORAGE_ENC_KEY` is a hex-encoded 32-byte key. `fs::put` is **write-tmp-then-atomic-rename + fsync** so a crash mid-write can't leave a torn blob. User-supplied filenames pass through `handlers::sanitize_filename` before becoming object keys (the original name is kept in the DB for display).

### ID strategy (hybrid — see `migrations/0001_init.sql` header)

- **System tables** (`systems`, `orgs`, `folders`, `views`, `users`) → `TEXT` PKs, app-generated CUID2. Seed rows use readable literals (`sys_hr`, `org_phattana`) so URLs and test asserts stay debuggable.
- **Transaction tables** (`files`, `activity`, `sessions`, `share_links`, `comments`, `notifications`, `workflow*`, `file_versions`, `thumbnails`) → `UUID` PKs, UUIDv7 for insert-order index locality.

FK column types follow the referenced PK type. Models in `models.rs` reflect this: `System.id: String`, `File.id: Uuid`.

## Basket platform (Nov 2026)

- A **Basket** is a `systems` row (UI name only). Per-basket settings live in `basket_categories`, `basket_fields` (values in `files.metadata` JSON), `basket_statuses` + `basket_status_transitions`, `basket_policies` (basket or folder: encrypt, allowed extensions, max size, public links) — `basket.rs`.
- **Access** (`acl.rs`): `acl_entries` grant View/Edit/Manage on basket | folder | file to `user:`, `org:`, `role:`, `iamx:` principals or `everyone`. The SQL function `fh_level()` (migration 0018) is the single rule: basket level = basket grants if the basket has any, else the role (admin 3 · editor 2 · viewer 1); folder and file grants add on top; a folder with `inherit=false` cuts everything above; an uploader keeps Edit on their file; the owner of a personal drive has Manage. List queries filter with `fh_level(...) >= 1`; handlers call `acl::require_file` / `acl::require_folder`. Edit may *forward* (add View grants) but only Manage changes access.
- **Virtual Baskets** (`vbasket.rs`): saved Sources + Rules (match all/any, one nested group) + Group by + Drop target, compiled to bound SQL at read time; nothing is copied. Download lists zip files from many places.
- New screens live in `frontend/app/(cx)` + `frontend/components/cx` with `app/carbon.css` (IBM Carbon g10 tokens from the MaintainX design system, compact density, `cx-` prefixed). The older screens still work at their old routes; `/` opens `/recent`, the old dashboard is at `/overview`.

## Frontend architecture

- `app/` — App Router pages. Routes map 1:1 to the screens in the README (`/`, `/files` + layout variants `board`/`gallery`/`calendar`/`timeline`, `/files/[id]`, `/upload`, `/views`, `/views/new`, `/orgs`, `/share`, `/settings` + sub-pages `members`/`roles`/`audit`, `/login`, `/trash`, `/activity`, `/archive`).
- `components/` — `sidebar.tsx`, `topbar.tsx`, `primitives.tsx` (shared UI), `icons.tsx` (40+ stroke icons inline), `notifications-bell.tsx`, `global-search.tsx`, `user-menu.tsx`, `pager.tsx` (shared pagination for the list pages).
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
- `RUST_LOG` — tracing filter.
- `STORAGE_ENCRYPT_ALL` — `true` (default) encrypts every object when `STORAGE_ENC_KEY` is set; `false` encrypts only where a basket/folder policy asks or a file is encrypted on demand (`POST /api/files/:id/encrypt`).
- `S3_PORT` — when set (e.g. `9000`), a second listener serves an S3-compatible API at its root (`s3.rs`) so systems that wrote to MinIO can write here. Bucket = basket storage key; keys are created in Admin → S3 access keys and act as a FileHub user. `S3_CRED_KEY` (falls back to `STORAGE_ENC_KEY`, then `WOPI_SECRET`) seals the stored secrets — changing it invalidates existing keys.
- `IAMX_JWKS_URL` / `IAMX_JWT_ISSUER` / `IAMX_ALLOWED_CLIENTS` — accept an IAMX/Keycloak access token as `Authorization: Bearer <JWT>` (another system calling as the signed-in person; `auth::bearer_user`). Unset JWKS = such tokens are refused. Test: `scripts/iamx-bearer-check.py`.
- `EDGE_JWKS_URL` / `EDGE_JWT_ISSUER` — when set, the IAMX/Keycloak access token forwarded by the edge must verify against these keys (`jwks.rs`), on top of the `x-filehub-edge` secret.

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

## Legacy `/FileService/*` compatibility (`backend/src/legacy.rs`)
Lets older apps that call the legacy FileService API
use FileHub with **no code change** — only point their center URL / edge route at FileHub.
Endpoints: `upload`, `uploadFile`, `downloadFile`, `previewFile`, `getFileDetail`, `getFiles`, `rename`, `moveFileToTrash` (served at the root, outside `/fh`).
- Identity: edge SSO first (the person, even though old apps also send an unverifiable center token) → IAMX token / `fhk_` key → legacy service account `usr_legacy_fileservice` (turn off with `LEGACY_TRUST_CALLER=0`).
- Same rules as FileHub: per-file/folder access, basket policy, stamped downloads, Vault logging, locked status on rename.
- Folders keep their old `parentFolderId` as the FileHub folder id; files land in the basket from `x-filehub-system`, `LEGACY_SYSTEM_MAP` (app path → basket, from Referer), the person's default, or `LEGACY_DEFAULT_SYSTEM`.
- Old ids not in FileHub are fetched from the old service when `LEGACY_FILEHUB_URL` is set.
- Env: `LEGACY_FILESERVICE=1` (off by default), `LEGACY_DEFAULT_SYSTEM`, `LEGACY_SYSTEM_MAP=hr-portal=sys_x,...`, `LEGACY_FILEHUB_URL`, `LEGACY_TRUST_CALLER`, `LEGACY_JWT_KEY_URL` / `LEGACY_JWT_PUBLIC_KEY` (issuer key; tokens it signs pass the trust-callers connector through the ingress), `LEGACY_MAX_UPLOAD_MB`, `LEGACY_UPLOAD_RATE_PER_MIN`.
- Never expose `/FileService/*` to the internet without the edge in front.
- Test: `scripts/legacy-fileservice-check.py` (16 checks, written from the real callers' request shapes).

## Scale-out and MinIO-style features
**Many pods.** Everything shared lives in Postgres or `SPOOL_DIR`:
- `SPOOL_DIR` — uploads in progress (TUS chunks `.tus/`, S3 parts `.s3/`). One volume shared by all pods (RWX). Defaults to `STORAGE_ROOT`.
- Rate limits are counted in the UNLOGGED `rate_hits` table (`ratelimit.rs`), so they hold across pods. `UPLOAD_RATE_PER_MIN` (default 60 per person).
- Webhook worker and rotation use `SKIP LOCKED` / idempotent queries — safe on every pod.

**Bursts of uploads** (`admission.rs`): `admit_uploads` (outermost middleware) lets `UPLOAD_REQUESTS` upload requests per pod in at once (default 4 × `DATABASE_MAX_CONNECTIONS`), before any database work; inside, `upload_slot` lets `UPLOAD_CONCURRENCY` run their database phase (default = pool size). Waiters past `UPLOAD_QUEUE_WAIT_SECS` (300) get 503 + Retry-After, never a 500.
- `DATABASE_MAX_CONNECTIONS` per pod (default 32) — pods × this must stay under Postgres `max_connections` (use PgBouncer for many pods).
- `DATABASE_PING_BEFORE_USE` (default on) — keep on: without it, half-dead connections after a spike fail request after request.
- `STORAGE_FSYNC` (default on) — every stored object is fsynced. `off` only for benchmarks (macOS fsync is a full-device flush that hides app limits).

**Write path cost** (per upload, measured): ~20 short statements, one commit (file row + activity in one CTE). Quota totals come from a per-pod stale-while-revalidate cache (`handlers::cached_sum`) — never `SUM(files)` per upload.

**S3 API** (`s3.rs`): versioning (`versionId` on GET/HEAD, ListObjectVersions, `x-amz-version-id`), ListParts, ListMultipartUploads, tagging (stored as `key:value` FileHub tags → searchable), object lock (`?retention`, `?legal-hold`, bucket `?object-lock` read), streamed CopyObject. Still 501: ACL, bucket policy, CORS config, replication, notifications API (use webhooks), lifecycle API (use rotation policies).

**Object lock** (`objectlock.rs`, `object_locks`): COMPLIANCE (no one can delete or shorten), GOVERNANCE (admin may), legal hold. Enforced on every delete path (web, S3, legacy, folder delete, rotation). Basket/folder policy `lock_days` + `lock_mode` locks new files. REST: GET/PUT `/api/files/:id/lock`.

**Webhooks** (`webhooks.rs`): events `file.uploaded|version|deleted|restored|status` go to an outbox in the request; a worker POSTs them signed `X-FileHub-Signature: sha256=<HMAC>` with backoff, up to 10 tries. Admin REST `/api/webhooks`.

**Metrics**: `/fh/metrics` (Prometheus) when `METRICS_TOKEN` is set (Bearer). Requests and latency by kind, upload queue, DB pool, files and bytes.

**Import from MinIO/S3**: POST `/api/admin/import-s3` {endpoint, bucket, prefix, access_key, secret_key, system_id} → job; GET `/api/admin/import-s3/:id` progress. Keys become folders; re-runs skip what's there.

Tests: `scripts/s3-features-check.py` (boto3), `scripts/load/burst` (N uploads within RAMP s), `scripts/load/upload-burst.js` (k6 ramp).

## Capacity for 8,000 people uploading at once (measured on a laptop, Nov 2026)
Test: `scripts/load/burst` — N people each upload one 200 KB file within RAMP seconds, 2 FileHub instances, Postgres 15 (default config), fsync on.
| Burst | Result | Slowest |
|---|---|---|
| 8,000 within 30 s | 8,000/8,000 OK, 0 errors | 17 s |
| 8,000 within 5 s | 8,000/8,000 OK on a clean run | 42 s |
Throughput ≈ 150–370 uploads/s on the laptop; the limits there are macOS (fsync = full-device flush, loopback TCP stalls with tens of thousands of sockets, listen backlog 128), not FileHub. Repeating the 5 s burst back-to-back on the Mac fails client-side (ephemeral ports / loopback stalls) — server metrics show nothing stuck.

Production sizing for 8,000 concurrent (starting point; confirm with the same test on the real cluster):
- FileHub: 3–4 pods × 2 vCPU / 2 GB, `DATABASE_MAX_CONNECTIONS=30`, behind an ingress (nginx) that buffers request bodies (`proxy_request_buffering on`, `client_max_body_size` = largest file) — slow clients then never reach a pod.
- Postgres: 8 vCPU / 16 GB, SSD, `max_connections ≥ pods × 30 + 20` or PgBouncer (transaction mode), `shared_buffers 4GB`, `wal_buffers 64MB`, `commit_delay 200`.
- `SPOOL_DIR` on a shared RWX volume; object storage on SSD or S3 (`STORAGE_BACKEND=s3`).
- Network: 8,000 × average file size must cross the link — e.g. 8,000 × 5 MB = 40 GB; at 1 Gbit/s that alone is ~5.5 minutes, so the link, not FileHub, sets the time for big files.
- Watch `/fh/metrics`: `filehub_uploads_waiting` (queue), `filehub_uploads_in_step`, `filehub_db_pool_connections`, 5xx rate.
- `UPLOAD_IDLE_TIMEOUT_SECS` (default 60): an upload whose client sends nothing for this long ends with 408, so stalled clients never hold a connection forever.
- Errors: every API error body is `{ "error": <sentence for people>, "code": <stable id> }` — the web app shows `error` as is.

## Mounted folders (mount.rs)
A basket can mirror a folder mounted into the pod (Docker volume / NFS / SMB).
- `MOUNT_ROOTS` — comma-separated absolute folders that may be mounted (resolved; symlinks out are refused). Empty = feature off.
- `MOUNT_SCAN_SECS` — background scan interval (default 300, 0 = off; one pod scans at a time via advisory lock).
- Keys of such files are `mount:/abs/path`; files stay plain (never encrypted), uploads land under their real name, old versions move into FileHub storage. Vaults can't be mounted.
- Admin → basket settings → "Mounted folder"; `PUT /api/baskets/:id/mount`, `POST /api/baskets/:id/scan`. Test: `scripts/mount-check.py`.

## Connectors (connectors.rs)
Old `/FileService` callers are set up in Admin → Connectors (token, path prefixes, basket, role, read-only). `LEGACY_*` env is only imported once into connectors on first start.
