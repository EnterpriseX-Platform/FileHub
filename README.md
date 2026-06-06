# File Hub

DevOps-grade file management system built from a [Claude Design](https://claude.ai/design) handoff bundle.

- **Frontend** — Next.js 15 (App Router, TypeScript), Linear × Notion aesthetic, 10 connected screens
- **Backend** — Rust + Axum + SQLx (SQLite metadata) + local-filesystem object store
- **Scale target** — 7 systems × 8,247 organizations × millions of files
- **Dependencies** — none beyond the Rust toolchain and Node. No Docker, no MinIO, no external storage.

## Quick start

```bash
# 1. Run backend (port 8090). Creates ./storage and filehub.db on first start.
cd backend && cargo run

# 2. Run frontend (port 3000). Override with PORT=3001 if 3000 is taken.
cd frontend && npm install && npm run dev
```

Open <http://localhost:3000>. The backend proxy is configured via `frontend/next.config.ts`.

## Screens

| # | Route | Description |
|---|-------|-------------|
| 01 | `/`                  | Dashboard — stats, storage breakdown, recent activity, pinned views |
| 02 | `/files`             | Files · Table view — Notion-style metadata table |
| 03 | `/files/board`       | Files · Board view — Kanban grouped by status |
| 04 | `/files/gallery`     | Files · Gallery view — large thumbnails, date grouping |
| 05 | `/files/[id]`        | File detail + inspector — preview, metadata, activity |
| 06 | `/upload`            | Upload flow — dropzone, queue, auto-metadata |
| 07 | `/views/new`         | View builder — saved-view editor with live preview |
| 08 | `/orgs`              | Orgs & Systems — manage 7 systems × 8,247 orgs |
| 09 | `/share`             | Permissions & sharing modal |
| 10 | `/settings`          | Workspace settings |

## Backend API

| Method | Path | Description |
|---|---|---|
| GET    | `/api/health`             | Liveness check |
| GET    | `/api/stats`              | Dashboard stats |
| GET    | `/api/systems`            | List the 7 connected systems |
| GET    | `/api/orgs?system_id=…`   | List orgs (filterable by `system_id`) |
| GET    | `/api/files?…`            | List files (filterable by `system_id`, `org_id`, `status`, `project`) |
| GET    | `/api/files/:id`          | File detail with metadata |
| POST   | `/api/files`              | Multipart upload → filesystem + DB + activity row |
| GET    | `/api/files/:id/download` | Stream file content |
| DELETE | `/api/files/:id`          | Remove file from disk + DB |
| GET    | `/api/activity?limit=…`   | Recent activity feed |
| GET    | `/api/views`              | Saved views |
| POST   | `/api/views`              | Create a saved view |
| GET    | `/api/permissions/:fileId`| Permissions for a file |

CORS is open in dev (`http://localhost:3000`). In production the frontend proxies `/api/*` to the backend via `next.config.ts` rewrites, so the browser only talks to the Next.js origin.

## Storage layout

```
backend/
├── filehub.db                 # SQLite metadata (created on first run, gitignored)
└── storage/                   # Files on disk, created on first run, gitignored
    ├── hr-emp-files/          # one directory per bucket
    │   └── file-<uuid>-<filename>
    ├── fin-invoices/
    ├── legal-contracts/
    └── …
```

ETag is the MD5 hex of the uploaded body. Configurable via env: `STORAGE_ROOT=/var/lib/filehub/storage`.

## Layout

```
FILEHUBNEW/
├── README.md
├── backend/
│   ├── Cargo.toml
│   ├── .env.example
│   ├── migrations/            # 0001_init.sql, 0002_seed.sql
│   └── src/
│       ├── main.rs            # axum router + CORS + tracing
│       ├── state.rs           # SQLite pool + storage init
│       ├── storage.rs         # local filesystem object store (put/get/delete)
│       ├── error.rs           # IntoResponse error type
│       ├── models.rs          # DB rows + DTOs
│       └── handlers.rs        # all API handlers
├── frontend/
│   ├── app/                   # 10 design screens + 2 stub routes (activity, archive)
│   ├── components/            # sidebar, topbar, primitives, 40+ stroke icons
│   └── lib/                   # typed fetch client + format helpers
└── scripts/
    └── test-api.sh            # end-to-end API test suite
```

## Testing the API

```bash
# With the backend running on port 8090
./scripts/test-api.sh
```

The script runs 15+ tests covering health, stats, listing, filtering, upload, download, delete, and activity.
