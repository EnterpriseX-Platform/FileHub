# Prototype Port Checklist — the harness

Goal: **every feature, both shells, rebuilt in the New Prototype Concept**
(Calm warm-paper language; spec = `frontend/public/everyday-prototype.html`
and `admin-console-prototype.html`). Rules of the port:

1. **Features always win.** Where the prototype (a demo) lacks a real feature
   (bell, grid/list toggle, RBAC gating, bilingual, personal drives), the
   feature stays and gets prototype styling.
2. **No simulated UI in the real app.** Prototype set pieces that imply
   nonexistent features (presence, "draft a memo") become roadmap features,
   not fake buttons.
3. Every ✅ below was verified in the browser (DOM assertion or inspected
   screenshot), not just compiled.

Status: ✅ ported+verified · 🔧 in progress · ⬜ pending · 🎨 inherits tokens,
needs visual QA/alignment pass · 🚧 feature-level (build, don't fake)

## Global / design system

| Item | Status | Notes |
|---|---|---|
| Calm warm-paper tokens (light+dark) | ✅ | `tokens.css` `c562263` |
| Ink primary buttons / on-accent flip | ✅ | |
| Gray active-nav states | ✅ | |
| Ink squircle favicon + brand mark | ✅ | |
| Toasts / skeletons / empty states / thumbnails | ✅ | earlier polish pass |
| Everyday-first routing + "Admin console" naming | ✅ | |
| Stagger entrance motion + view slide | ✅ | home lists/tiles/areas + eday-main slide |
| ⌘K palette grouped layout (Pages/Files/Ask-AI) | ✅ | real palette already groups (actions + pages) and is functionally richer — accepted as parity |

## Everyday shell

| Surface | Status | Notes |
|---|---|---|
| Topbar (62px glass, pill nav, ✦ Ask, ⌘K pill) | ✅ | `06a75e5` |
| Home hero + ask bar + chips + brief | ✅ | brief types in (reduced-motion safe) |
| "Waiting on you" human rows + Review → | ✅ | now also lists signature turns (GET /sign-requests/mine) with Sign → — every role |
| Signature-mark library (upload + sign with mark) | ✅ | SignPanel: saved marks, upload image → /api/signatures, sign sends signature_id |
| Tiles ("when · area", flush thumb) | ✅ | |
| Area tiles (tone dot + real counts) | ✅ | |
| My files / Shared / Recent / Starred + area chips | ✅ | |
| File view: header links, AI card, steppers | ✅ | steppers shared w/ sidecar |
| /ask conversation + typewriter + round send | ✅ | |
| /ask agentic "Open {source} →" chip | ✅ | navigates to /f/{top-source} |
| Bottom mobile nav ✦ Ask tab | ✅ | replaces Starred slot (Starred stays in top nav + /starred) |
| Upload page (everyday entry) | ✅ | swept — ink dropzone, Calm chrome |
| Login page | ✅ | hero retoned navy→warm ink; violet kept only as AI accent (light+dark shot) |
| Notifications dropdown | ✅ | glass pass done |

## Admin console (Workspace)

| Surface | Status | Notes |
|---|---|---|
| Dashboard (strip, review queue, activity groups, donut, pinned chips) | ✅ | declutter pass |
| Dashboard inline Approve on queue rows | ✅ | app/review-queue.tsx — verified live (row exit + toast) |
| Dashboard "ask about your workspace" bar | 🚧 | needs analytics-RAG; don't fake — roadmap |
| Files table (dot statuses, calm rows) | ✅ | |
| Files board/gallery/calendar/timeline layouts | ✅ | swept in browser — all four Calm |
| File detail (workspace) + sidecar | ✅ | steppers landed via shared components |
| Upload | ✅ | swept |
| Reports: KPI cards | ✅ | AI-usage panel (asks/summaries/embeddings + totals) from real metering, admin-only; prototype's OCR/signature counts have no backing metric yet → 🚧 backlog |
| Reports: status bars + CSV exports | ✅ | existing, calm-inherited |
| Reports: document-status table pagination | ✅ | shared Pager, 15/page |
| Workflow template builder — visual flow diagram | ✅ | ActivePieces-style vertical nodes, + insert, drag-reorder, parallel fan-out (/settings/workflows) |
| Settings: general / members / roles / workflows / audit | ✅ | swept — all five Calm; workflows now flow-diagram |
| Orgs / Share / Views / Views-new | ✅ | swept (views/new also crash-fixed) |
| Trash / Activity / Archive / Search | ✅ | swept — semantic search violet bars = chroma-as-information |
| Sidebar + user card | ✅ | |

## Feature-level (roadmap, not ports)

| Item | Status |
|---|---|
| **Document viewer: annotation + signature stamp** | 🚧 spec below |
| Workflow conditional branches in the flow diagram (ANNEX-14w) | 🚧 needs backend rules model |
| Presence ("X is viewing") — backend heartbeat | 🚧 |
| True SSE streaming for /api/ask | 🚧 |
| Conditional workflow routing (ANNEX-14w) | 🚧 |
| Metadata CSV import / template folders / bulk watermark / PDF annotate | 🚧 |
| PAdES byte-embed + PKI certs | 🚧 |
| Claude Design publish (user must run /design-login) | 🚧 |

## Spec: annotation + signature-stamp viewer (user-requested)

Today's viewer is Chrome's PDF plugin in an iframe — nothing can be drawn on
it. The real build:
1. **Foundation**: render PDFs with pdf.js onto canvases (replace the iframe in
   `app/files/[id]/preview.tsx`), with a positioned overlay layer per page.
2. **Annotations** (TOR ANNEX-3): highlight + note objects on the overlay;
   new `annotations` table (file_id, page, rect, kind, body, author) + CRUD
   endpoints; render on load; permission = same as comments.
3. **Signature stamp**: place a mark from the existing e-sign signature library
   onto a page — the esign data model ALREADY stores placement (page/x/y/w/h),
   so stamping = drag the saved mark onto the overlay and create/complete a
   sign-request with that placement. Server-side bake into the PDF bytes is the
   PAdES work item (ANNEX-34) and can come after the visual layer.

## The 🎨 sweep — how to close it

For each 🎨 page: open it in the browser under Calm (light+dark, EN+TH),
compare against the prototype's closest analog, fix spacing/chrome to the
prototype metrics (46px section rhythm, 19px/700 headers, pill controls,
hairline borders), verify with the Playwright rig, tick to ✅ here in the
same commit.
