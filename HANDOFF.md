# FileHub — Session Handoff

**Branch:** `feat/ai-native` (pushed, up to date)
**Last commit:** `06a75e5` — verbatim prototype parity across the everyday shell
**Read `CLAUDE.md` first** — kept current through this engagement.

Two engagements are layered here:
1. **TOR gap closure** (OCR, e-sign, workflow engine, etc.) — see the TOR section below.
2. **Design overhaul + AI activation** (this session): the product now runs the
   **Calm (Notion-like warm paper)** design language, **Everyday-first** routing,
   and a **live AI layer** (Kimi chat + local Ollama embeddings).

---

## Current state (start here)

- All code committed + pushed. Untracked on purpose: `FileHub-TOR-compliance-matrix.xlsx`
  (bid artifact) and `frontend/public/{everyday,admin-console}-prototype.html` +
  `facelift-concept.html` (design review artifacts, gitignored — the **design spec**;
  keep them, the port harness references them).
- Backend suite was 60/60 green at last full run. Local dev DB is seeded + has 6 rich
  bilingual demo docs (TOR-summary, minutes, HR policy, vendor eval, security guideline,
  budget memo) that make AI demos meaningful.
- **AI is LIVE**: `backend/.env` (gitignored) holds a Moonshot **Kimi key**
  (`AI_BASE_URL=https://api.moonshot.ai/v1`, `AI_CHAT_MODEL=kimi-k2.6`) with
  **embeddings on local Ollama** via the new `AI_EMBED_BASE_URL` split (Moonshot has no
  embeddings API). Ollama is installed as a user app (auto-starts, port 11434) with
  `nomic-embed-text` pulled. Treat the key as test-grade (it passed through chat) —
  rotate before any real deployment.
- Seed blobs were materialised (`scripts/seed-bodies.sh`) so previews work.
- Playwright Chromium is installed under the frontend — the browser-verification rig
  (see "How to verify UI" below).

## Design system — the decisions that bind future work

- **Chosen language: Calm** — Notion-like warm paper (light `#fdfdfb` ivory / dark
  `#191817` warm charcoal), **ink primary buttons** (`--grad` is an ink gradient token;
  `--on-accent` flips per mode), gray active-nav states, no glows. Chroma is information
  only: status dots, file-type badges, iris links/focus, violet AI sparkle.
- Chosen via 4-variant clickable prototypes (Vivid / Hybrid / Warm-hybrid / Calm — the
  ◎ button in the prototypes cycles them). **The prototypes are the design spec**:
  open `http://localhost:3001/filehub/everyday-prototype.html` and
  `admin-console-prototype.html` (cross-linked). Prototype source parts live in the
  session scratchpad only — the built HTML files in `frontend/public/` are canonical now.
- **Everyday-first routing**: every role (admins included) defaults to `/home`;
  the Workspace is the "Admin console" entered via the user menu. `lib/view-mode.tsx`
  + `app/page.tsx`.
- Typography: Inter + IBM Plex Sans Thai + JetBrains Mono. Brand mark: ink squircle F
  with violet sparkle (`frontend/app/icon.svg`).
- **Naming hazard learned the hard way**: theme/mode classes on `<html>` collided with a
  component class (`.paper` was the file-preview container) — mode classes must use
  reserved names.

## Everyday shell — verbatim-parity status

Ported 1:1 from the prototype (commit `06a75e5`): 62px glass topbar + pill nav
(+ ✦ Ask item), ⌘K pill, two-line 800-weight hero, hairline ask ring + round ink send,
suggestion chips, data-honest daily brief, "Waiting on you" human-sentence rows with
`Review →` links, "Pick up where you left off" tiles (flush 16:10 thumb, "when · area"),
tinted area tiles with real counts, area filter chips on My files/Shared/Recent/Starred,
file-view Intelligence card (model attribution) + **stepper** Signatures/Workflow
(all actions preserved; steppers shared with the workspace sidecar), conversation-thread
/ask with typewriter reveal + round send.

## AI layer — verified end-to-end this session

- Enrichment worker processed all docs (0 failures); per-file summaries/tags render with
  `kimi-k2.6` attribution; semantic search ranks Thai meaning-queries correctly;
  `/api/ask` returns grounded cited answers (Thai + EN).
- **Two RAG bugs found via real-provider testing and fixed** (`2f20548`): ask deduped to
  one chunk per file (evicting the chunk containing the answer — now 2/file merged into
  one citation) and `ASK_CHUNK_CHARS` 700→1600 (answers in a chunk's back half were
  silently truncated).
- AI usage metering has real rows (`GET /api/reports/ai-usage`, admin).
- Good demo questions: "งบประมาณโครงการ DMS เท่าไหร่ เบิกจ่ายไปแล้วเท่าไร",
  "พนักงานลาพักร้อนได้กี่วัน", "Who won the CCTV vendor evaluation?".

## Remaining work — see `PORT-CHECKLIST.md` (the port harness)

The rebuild-everything-on-the-prototype effort is tracked feature-by-feature in
**`PORT-CHECKLIST.md`** at the repo root. Headline items:
1. Everyday finishing: agentic open-source chip on /ask, stagger motion, typed brief,
   bottom-nav ✦ Ask, ⌘K grouped layout.
2. Console finishing: dashboard inline Approve (real PATCH), Reports KPI cards.
3. Workspace page sweep: every `/`-side page inherits Calm tokens but needs a visual QA
   + prototype-alignment pass (list in the checklist).
4. Feature-level (not ports): presence indicator (needs backend heartbeat — do NOT fake),
   true SSE streaming for /ask, conditional workflow routing (ANNEX-14w), metadata CSV
   import, template folders, bulk watermark, PDF annotate, PAdES/PKI.
5. **Claude Design publish** — blocked on the user running `/design-login` in an
   interactive terminal; the bundle is the design-system HTML already delivered.

## How to run

```bash
docker compose up -d postgres
cd backend && cargo run          # reads backend/.env — AI ON by default now
cd frontend && npx next dev -p 3001
```
Open <http://localhost:3001/filehub>. Logins: `admin@acme.go.th`/`admin123`,
`anong@acme.go.th`/`anong123` (editor), `viewer@acme.go.th`/`viewer123`.

## How to verify UI (the rig that works)

The preview MCP screenshot tool is unreliable — use **Playwright headless** instead
(Chromium already installed via the frontend):
```js
const { chromium } = require(require.resolve("@playwright/test", { paths: ["D:/Claude/FileHub/frontend"] }));
// login via fetch POST /filehub/api/auth/login inside page.evaluate, then goto/screenshot/assert
```
Assert DOM facts (classes, text) + inspect screenshots. Pattern examples lived in the
session scratchpad (`shot-*.js`, `qa*.js`) — rewrite freely, it's ~30 lines.

## Gotchas (also in auto-memory)

- **Never `npm run build` while `next dev` is running** — clobbers `.next`, dev server
  500s with "Cannot find module './NNN.js'". Stop dev → build → restart.
- **Windows exe-lock**: build the backend test exe (`cargo test --test api --no-run`)
  while the backend is STOPPED; run tests via the prebuilt
  `./target/debug/deps/api-*.exe --test-threads=1`. Stale backends squat port 8090.
- Git Bash → Windows Python: pass paths as **argv** (MSYS converts them); paths inside
  `-c` strings don't convert.
- Frontend port is always 3001; server-side fetches use `127.0.0.1`, never `localhost`.

## TOR engagement summary (previous phase — unchanged facts)

OCR (`ocr.rs`), e-signature (`esign.rs`, mig 0019), SMTP (`mailer.rs`), boolean search,
version restore, session idle-timeout, configurable workflow engine (`workflow.rs`,
mig 0020) + full workflow UI, workflow templates at `/settings/workflows`.
Compliance matrix `FileHub-TOR-compliance-matrix.xlsx` (~55% Comply / 27% Partial).
Deferred integrations: AD SSO, e-HR, EngDoc/INFOMA, Service Gateway.
Migrations at **0020**; next is `0021_*`. `sqlx::migrate!` checksums forbid editing
applied migrations — reset dev DB if you must change an unshipped one.
