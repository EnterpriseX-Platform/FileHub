# File Hub

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](LICENSE)
[![Backend: Rust + Axum](https://img.shields.io/badge/backend-Rust%20%2B%20Axum-orange.svg)](backend)
[![Frontend: Next.js 15](https://img.shields.io/badge/frontend-Next.js%2015-black.svg)](frontend)

A self-hostable document & file-management system: organize files across multiple
source systems and organizations, preview and edit Office documents in the browser,
share via tokenized links, and keep an audit trail — with role-based access control
and optional at-rest encryption.

Backend is **Rust + Axum + Postgres**; frontend is **Next.js 15** (App Router,
TypeScript). Object storage is pluggable — a local filesystem or any S3-compatible
service (AWS S3, **MinIO**, Wasabi, R2) — with optional transparent AES-256-GCM
encryption layered in front.

## Features

- **File management** — multipart + TUS resumable upload, download, soft-delete + trash,
  folders, versions, tags, and metadata across table / board / gallery / calendar /
  timeline views.
- **Multi-tenant model** — files scoped to *systems* (source apps) and *organizations*,
  plus personal "My Drive" spaces; per-resource authorization on every id.
- **Auth & RBAC** — session cookies for browsers and `Bearer` API keys for
  machine-to-machine; roles `admin` / `editor` / `viewer` enforced server-side; argon2id
  password hashing with login throttling.
- **Office editing** — optional in-browser editing via Collabora Online (the backend is a
  WOPI host); PDF-converted previews + text extraction otherwise.
- **Pluggable storage** — filesystem (default) or S3-compatible (hand-rolled SigV4, no
  `aws-sdk-s3`); optional AES-256-GCM encryption at rest.
- **Sharing** — tokenized public share links (the token is the credential).
- **Lifecycle** — background rotation worker for version pruning, archival, and
  hard-delete by configurable retention rules.
- **Search, activity & reports** — full-text search (including extracted PDF/text
  content), an activity feed, and storage/usage reports.

## Tech stack

| Layer | Tech |
|---|---|
| Backend | Rust, Axum, SQLx, Postgres |
| Frontend | Next.js 15 (App Router), React 18, TypeScript |
| Storage | filesystem or S3-compatible (AWS S3 / MinIO / Wasabi / R2) |
| Office | Collabora Online (optional) |
| Infra | Docker Compose — Postgres, optional MinIO + Collabora |

## Quick start

Prerequisites: a Rust toolchain, Node 18+, and Docker (for Postgres).

```bash
# 1. Start Postgres (host port 5434).
docker compose up -d postgres

# 2. Backend on :8090 — runs migrations and seeds demo users on first start.
#    Debug builds have dev-friendly defaults; no .env needed to get going.
cd backend && cargo run

# 3. Frontend on :3001 (the rest of the system expects port 3001).
cd frontend && npm install && npm run dev -- -p 3001
```

Open <http://localhost:3001/filehub>.

Demo logins — seeded **only when the `users` table is empty**, so change them before any
real deployment:

| Email | Password | Role |
|---|---|---|
| `admin@acme.go.th`  | `admin123`  | admin  |
| `anong@acme.go.th`  | `anong123`  | editor |
| `viewer@acme.go.th` | `viewer123` | viewer |

### Optional: S3 / MinIO storage

```bash
docker compose up -d minio minio-setup   # S3 API on :9000, console on :9001
```

Then set `STORAGE_BACKEND=s3` and the `S3_*` block in `backend/.env` (see
[`backend/.env.example`](backend/.env.example)). MinIO requires path-style addressing —
keep `S3_PATH_STYLE=true` (the default).

### Optional: in-browser Office editing

`docker compose up -d collabora` starts a branded Collabora Online container (port 9980).
Requires `WOPI_SECRET` to be set. For TLS and production notes, see the Production
checklist in [CLAUDE.md](CLAUDE.md).

## Configuration

All backend configuration is via environment variables — see the annotated
[`backend/.env.example`](backend/.env.example). The most important:

| Variable | Purpose |
|---|---|
| `DATABASE_URL` | Postgres connection string (**required in release**) |
| `STORAGE_BACKEND` | `fs` (default) or `s3` |
| `STORAGE_ENC_KEY` | hex-encoded 32-byte key → enables AES-256-GCM at rest |
| `CORS_ORIGIN` | exact browser-facing origin (**required in release**) |
| `WOPI_SECRET` | HMAC key for Collabora access tokens (**required in release**) |

Release builds (`cargo build --release`) fail-fast if `DATABASE_URL`, `CORS_ORIGIN`, or
`WOPI_SECRET` is unset.

## Architecture

The backend mounts every route under `/fh`; the frontend runs under basePath `/filehub`
and proxies `/filehub/api/*` → `${BACKEND}/fh/api/*` so cookies stay same-origin.
Authentication is enforced by a tower layer over the private sub-router; per-resource
authorization lives in the handlers. For a full tour of routing, the auth/authz model,
the ID strategy, and conventions, see [CLAUDE.md](CLAUDE.md).

```
.
├── backend/                  # Rust + Axum API
│   ├── migrations/           # SQLx migrations (Postgres)
│   ├── src/
│   │   ├── lib.rs            # router — single source of truth for routes
│   │   ├── auth.rs          # sessions, API keys, RBAC, login throttle
│   │   ├── handlers.rs      # files, systems, orgs, folders, shares, search, …
│   │   ├── p1.rs            # workflow, comments, notifications, previews
│   │   ├── tus.rs           # TUS 1.0.0 resumable upload
│   │   ├── wopi.rs          # WOPI host for Collabora
│   │   ├── rotation.rs      # retention / archive / hard-delete worker
│   │   ├── storage.rs       # AES-256-GCM encryption wrapper
│   │   └── store/{fs,s3}.rs # object-store backends
│   ├── openapi.yaml         # API spec
│   └── INTEGRATION.md       # external-app upload / integration guide
├── frontend/                 # Next.js 15 App Router
│   ├── app/                 # routes (dashboard, files, upload, settings, …)
│   ├── components/          # shared UI
│   └── lib/                 # typed API client
├── infra/collabora/          # branded Collabora image
├── scripts/                  # test-api.sh, backup.sh, restore.sh, seed-bodies.sh
└── docker-compose.yml        # Postgres + optional MinIO + Collabora
```

## API & integration

- Full spec: [`backend/openapi.yaml`](backend/openapi.yaml) — load into Swagger UI, Postman,
  or `redocly preview`.
- Integrating an external uploader (API keys, multipart + TUS, encryption):
  [`backend/INTEGRATION.md`](backend/INTEGRATION.md).

## Testing

```bash
# Backend integration tests run against a LIVE backend.
cd backend && cargo run        # one shell
cd backend && cargo test       # another shell

# End-to-end curl suite (also needs a running backend):
./scripts/test-api.sh
```

Liveness vs readiness: `/api/health` is a cheap static check; `/api/ready` does a real
DB + storage round-trip (use it for a Kubernetes `readinessProbe`).

Frontend: `npm run lint` and `npm run build`. (No frontend test runner is configured
yet — see [TODO.md](TODO.md).)

## Production

The defaults are dev-friendly. Before exposing the stack to real users, work through the
**Production checklist** in [CLAUDE.md](CLAUDE.md): rotate secrets, change the seed
credentials, enforce `COOKIE_SECURE` + TLS, replace the Postgres credentials, configure
backups (`scripts/backup.sh`), and run migrations as a one-shot job for zero-downtime
deploys.

## Contributing

Issues and pull requests are welcome. Before opening a PR, please make sure the backend
(`cargo test` + `cargo build --release`) and frontend (`npm run lint` + `npm run build`)
both pass. See [TODO.md](TODO.md) for the current backlog and [CLAUDE.md](CLAUDE.md) for
architecture and conventions.

## License

File Hub is licensed under the **GNU Affero General Public License v3.0** — see
[LICENSE](LICENSE) for the full text.

AGPL-3.0 is a strong copyleft license: if you run a modified version of File Hub as a
network service, you must make the corresponding source available to its users, and any
derivative work must also be licensed under AGPL-3.0.
