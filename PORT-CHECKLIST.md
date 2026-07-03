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
| "Waiting on you" human rows + Review → | ✅ | |
| Tiles ("when · area", flush thumb) | ✅ | |
| Area tiles (tone dot + real counts) | ✅ | |
| My files / Shared / Recent / Starred + area chips | ✅ | |
| File view: header links, AI card, steppers | ✅ | steppers shared w/ sidecar |
| /ask conversation + typewriter + round send | ✅ | |
| /ask agentic "Open {source} →" chip | ✅ | navigates to /f/{top-source} |
| Bottom mobile nav ✦ Ask tab | ✅ | replaces Starred slot (Starred stays in top nav + /starred) |
| Upload page (everyday entry) | 🎨 | dropzone hero done; verify under Calm |
| Login page | 🎨 | AI split hero predates Calm; tone-check panel |
| Notifications dropdown | ✅ | glass pass done |

## Admin console (Workspace)

| Surface | Status | Notes |
|---|---|---|
| Dashboard (strip, review queue, activity groups, donut, pinned chips) | ✅ | declutter pass |
| Dashboard inline Approve on queue rows | ✅ | app/review-queue.tsx — verified live (row exit + toast) |
| Dashboard "ask about your workspace" bar | 🚧 | needs analytics-RAG; don't fake — roadmap |
| Files table (dot statuses, calm rows) | ✅ | |
| Files board/gallery/calendar/timeline layouts | 🎨 | inherit tokens; QA pass |
| File detail (workspace) + sidecar | ✅ | steppers landed via shared components |
| Upload | 🎨 | |
| Reports: KPI cards | ✅ | AI-usage panel (asks/summaries/embeddings + totals) from real metering, admin-only; prototype's OCR/signature counts have no backing metric yet → 🚧 backlog |
| Reports: status bars + CSV exports | ✅ | existing, calm-inherited |
| Settings: general / members / roles / workflows / audit | 🎨 | members role selects exist; QA pass |
| Orgs / Share / Views / Views-new | 🎨 | |
| Trash / Activity / Archive / Search | 🎨 | |
| Sidebar + user card | ✅ | |

## Feature-level (roadmap, not ports)

| Item | Status |
|---|---|
| Presence ("X is viewing") — backend heartbeat | 🚧 |
| True SSE streaming for /api/ask | 🚧 |
| Conditional workflow routing (ANNEX-14w) | 🚧 |
| Metadata CSV import / template folders / bulk watermark / PDF annotate | 🚧 |
| PAdES byte-embed + PKI certs | 🚧 |
| Claude Design publish (user must run /design-login) | 🚧 |

## The 🎨 sweep — how to close it

For each 🎨 page: open it in the browser under Calm (light+dark, EN+TH),
compare against the prototype's closest analog, fix spacing/chrome to the
prototype metrics (46px section rhythm, 19px/700 headers, pill controls,
hairline borders), verify with the Playwright rig, tick to ✅ here in the
same commit.
