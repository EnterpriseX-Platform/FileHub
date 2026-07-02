# FileHub — Session Handoff

**Branch:** `feat/ai-native` (pushed to `origin/feat/ai-native`, up to date)
**Last commit:** `3267c9b` — configurable workflow engine
**Context:** This engagement extended FileHub (an AI-native document management system) to close gaps against a **scanned Thai TOR** for an Electronic Document Management System — *โครงการพัฒนาระบบจัดการเอกสารอิเล็กทรอนิกส์, เลขที่ จก. 3-2569*. The TOR PDF is at `C:\Users\akarapol\AppData\Local\Temp\Attach_TOR_1 (1).pdf` (43 scanned pages, no text layer).

Read `CLAUDE.md` first — it's the authoritative architecture guide and was kept current this engagement.

---

## Current state (start here)

- All code is **committed and pushed**. Working tree is clean except one untracked deliverable: `FileHub-TOR-compliance-matrix.xlsx` (see below) — not committed on purpose (bid artifact, not code).
- Backend suite: **60/60 integration tests green** (last full run).
- The local dev DB was **reset** this session (`DROP SCHEMA public CASCADE`) to re-run a corrected migration; it re-seeds on boot, so it's fine — just start fresh.
- Any long-running dev backend from this session may show `/ready` 500 (DB pool timeout from age) — kill it and `cargo run` again.

---

## What shipped this engagement (TOR gap closure)

20 commits (`128137b`→`3267c9b`). Two phases: (1) AI-native metering + a **two-persona UI redesign**, then (2) **TOR gap features**.

### TOR gap features (phase 2)
| Feature | TOR | Where | Tests |
|---|---|---|---|
| **OCR** (scanned img/PDF → searchable Thai+Eng) | ANNEX-5/7/25 | `backend/src/ocr.rs`; wired in `handlers::persist_upload`, `tus::finalise`, `ai_worker` | `ocr::tests` |
| **E-signature** (library, ordered/parallel signers, integrity hash, verify, expiry) | ANNEX-32/33/35/36/37/39 | `backend/src/esign.rs` (mig `0019`), UI `frontend/components/everyday/sign-panel.tsx` | `esign_*` (4) |
| **Email/SMTP** (pluggable, off by default) | ANNEX-17/38 | `backend/src/mailer.rs` (`lettre`); used by `esign` + workflow `notify_reviewer` | — |
| **Boolean/phrase search** (`websearch_to_tsquery`) | ANNEX-26 | `handlers::search_files` | `search_boolean_or` |
| **Version restore** | ANNEX-44 | `handlers::restore_version` → `POST /api/files/:id/versions/:v/restore` | `restore_version_roundtrip` |
| **Session idle-timeout** | ANNEX-9 | `auth.rs::idle_timeout_mins` + require_session; `SESSION_IDLE_MINUTES` | — |
| **Workflow engine** (sequential/parallel + turn-guard, no-code templates, send-back) | ANNEX-11/12/13w/15w/43 | `backend/src/workflow.rs` + `p1::start_workflow`/`decide_step` (mig `0020`) | `workflow_*` (3) |
| **Compliance matrix** | — | `FileHub-TOR-compliance-matrix.xlsx` (root) | — |

### AI-native + redesign (phase 1, earlier commits — already documented in CLAUDE.md)
- AI-usage metering (`ai_usage`) producer + admin report (`GET /api/reports/ai-usage`).
- **Two-persona UI**: `fh-view` cookie (`lib/view-mode.tsx`); **Everyday** consumer app under `app/(everyday)/` (`/home`, `/my`, `/shared`, `/recent`, `/starred`, `/f/[id]`) vs **Workspace** power app. Default by role (admin→workspace, else→everyday), toggle in both user menus.
- ⌘K command palette (`components/global-search.tsx`), Starred (`stars.rs`, mig `0018`), mobile bottom nav.

---

## Remaining work (prioritized) — **integrations are explicitly out of scope for now**

1. ~~**Workflow builder UI**~~ — **DONE (2026-07-02)**: shared `frontend/components/workflow-panel.tsx` (start from reviewers or template, sequential/parallel, turn-guarded approve/reject with note, send-back) wired into both the everyday `/f/[id]` view and the workspace `app/files/[id]/sidecar.tsx`; template editor at `/settings/workflows` (new settings tab). Backend `WorkflowStep` now also serializes the per-step `name` so template step labels render. Browser-verified end-to-end.
2. **Conditional routing** (ANNEX-14w) — route by decision/condition. Needs a rules model on top of the current linear steps (not yet started).
3. ~~**Frontend surfacing**~~ — **DONE (2026-07-02)**: version-restore button on the workspace sidecar version list (POST `/versions/:v/restore`, editors only); workflow panel in the everyday `/f/[id]` view (item 1).
4. **Small wins** (each ~contained): metadata CSV import (ANNEX-9p26), template folders (ANNEX-14), bulk watermark (ANNEX-20), PDF highlight/annotate (ANNEX-3).
5. **E-signature depth** (optional): visible PDF byte-embed (PAdES) + PKI digital certs (ANNEX-34, §26) — data model already carries placement + hashes.

**Explicitly deferred (integration phase):** Active Directory SSO, e-HR sync, EngDoc/INFOMA connectors, Service Gateway/DMZ (ANNEX-23/24/25i/31/45i).

---

## How to run the stack

```bash
# 1. Postgres (pgvector image; port 5434)
docker compose up -d postgres

# 2. Backend (:8090). AI off + rotation off is the safe dev combo:
cd backend && AI_ENABLED=false ROTATION_INTERVAL_SECS=0 RUST_LOG=warn cargo run

# 3. Frontend — MUST be port 3001 (CORS/redirects expect it); package.json says 3000:
cd frontend && npx next dev -p 3001
```
Open <http://localhost:3001/filehub>. Seed logins (`backend/src/auth.rs::SEED_ACCOUNTS`): `admin@acme.go.th`/`admin123`, `anong@acme.go.th`/`anong123` (editor), `viewer@acme.go.th`/`viewer123`. Seed user ids used in tests: `usr_admin`, `usr_anong`, `usr_viewer`.

New env vars this engagement (see `backend/.env.example`): `OCR_*`, `MAIL_*` (email off unless `MAIL_ENABLED=true`+`SMTP_HOST`), `SESSION_IDLE_MINUTES`.

Browser verification used the **Claude_Preview** MCP (`.claude/launch.json` defines `frontend` on 3001, `autoPort:false`). `.claude/launch.json` is gitignored.

---

## How to test (important gotchas)

Integration tests hit a **live backend** — start it first, then in another shell:
```bash
cd backend && cargo test            # all
```
**Windows exe-lock gotcha:** while the backend is running, `cargo test` fails to relink `filehub-backend.exe` ("Access is denied"). Workaround used all session:
```bash
cargo test --test api --no-run                 # build the test exe
./target/debug/deps/api-*.exe <filter> --test-threads=1   # run the prebuilt exe directly
```
Run serially (`--test-threads=1`) — the backend throttles logins per-process.

**Migrations:** `sqlx::migrate!` checksums forbid editing an applied migration. If you must change a *new/unshipped* migration, reset the dev DB so checksums match: `docker exec filehub-postgres psql -U filehub -d filehub -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"` then restart the backend (re-runs migrations + seed). Migrations are now at **0020**; next is `0021_*`.

---

## Session-specific learnings / gotchas (save time next session)

- **Local tools:** `pdftotext` present; **`pdftoppm` and `tesseract` are NOT installed** here — so OCR no-ops locally (by design). To actually OCR: install `tesseract-ocr tesseract-ocr-tha poppler-utils`. The pipeline is verified by unit tests + graceful-degradation, not by real OCR output.
- **`file_workflows.created_by` is TEXT** (users.id), not UUID — a latent bug decoded it as `Uuid` and 500'd on workflow completion; fixed in `c890ab4`/`3267c9b`. Watch this pattern: `users.id`/system-table PKs are **TEXT (CUID2)**, transaction tables are **UUID** (see CLAUDE.md ID strategy).
- **Full-text tsvector** uses the `simple` config; hyphenated names ("contract-A12.pdf") tokenize as a whole + parts, so bare-word tsquery tests need uploaded *content*, not seed filenames.
- **Seed files have DB rows but no blob bytes** → previews show `{"error":"not found"}`. Run `scripts/seed-bodies.sh` to populate if you need real previews.
- **Frontend port** is the recurring trap: `-p 3001`, always `127.0.0.1` (never `localhost`) for server→backend.
- Everyday file links point to `/f/[id]` (friendly view), not `/files/[id]` (technical).
- `FILE_COLS` in `handlers.rs` is `pub(crate)` (reused by `stars.rs`).

---

## Compliance matrix

`FileHub-TOR-compliance-matrix.xlsx` (repo root) — 74 requirements, filterable, color-coded **Comply / Partial / Custom-dev / Service-NA**, with a Summary sheet. Reflects everything built this engagement (regenerate with `scratchpad/genmatrix.py` if statuses change). Current posture ≈ **55% Comply / 27% Partial / 15% Custom-dev**. The two scored hard-requirements (OCR, e-signature) are **Comply**; AI semantic search + "Ask your documents" targets the **EVAL-9 AI bonus (+10 pts)**.
