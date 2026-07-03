# Requests — Form/Document → Approval, AI-first — E2E Results

**Date:** 2026-07-03
**Feature:** A new "Requests" area in the Everyday UI. A user starts a request by
**describing it in plain language** (or filling a form); the AI classifies the
request type, fills the form, and proposes an approval route; the request — with
its attached or auto-generated document — then flows through FileHub's **existing
approval engine** (`file_workflows`). Approvers act from "Waiting on you".

**Result: 31 / 31 checks passed** (live, against the running stack). The feature
reuses the workflow engine verbatim — no approval logic was duplicated.

---

## Architecture (maximum reuse)

- A **request anchors to a document** — the receipt/contract the user attaches, or
  a Markdown summary generated from the form — and runs through the existing
  `file_workflows` engine. So approvals, notifications, "Waiting on you", the
  sequential turn-guard, send-back, and audit are all **reused, not rebuilt**.
- New surface is deliberately thin: a `requests` overlay table (`migration 0022`),
  a static set of form schemas, a routing policy, one **AI intake** endpoint, and
  the Everyday screens. The workflow-start logic was refactored into a shared
  `p1::start_workflow_core` so files and requests start workflows through one path.
- Anchors live in a shared `sys_requests` area so **every assigned reviewer can
  read the document** (a personal drive would 403 them).

## Product decisions (as chosen)

- **AI picks the approval route, the requester can edit it** — policy proposes the
  reviewer chain (e.g. Expense: Manager → Finance, + Director when > ฿5,000); the
  requester can add/remove approvers before sending.
- **Anyone signed in can submit** a request (viewers included); the **approval
  steps remain editor+** (enforced by the reused decision handler).
- Four request types: Expense, IT / Service, Document approval, Leave.

---

## Test matrix

### A. Form schemas
| Check | Result |
|---|---|
| `GET /api/request-forms` returns the 4 kinds | ✅ |

### B. AI intake — plain language → structured draft
| Check | Result |
|---|---|
| "reimburse 4500 baht…" → classified **expense** | ✅ |
| amount extracted (4500) | ✅ |
| route proposed = Manager → Finance | ✅ |
| "need a new laptop…" → classified **it** | ✅ |
| "approve the vendor contract…" → classified **document** | ✅ |

### C. Create + policy routing
| Check | Result |
|---|---|
| create expense (no attachment) → 200, `in_review` | ✅ |
| a **summary `.md` anchor document is generated** | ✅ |
| policy route = Manager, Finance | ✅ |
| expense **> ฿5,000 adds a Director** step | ✅ |
| caller's **reviewer override is honored** | ✅ |

### D. List boxes + turn order
| Check | Result |
|---|---|
| requester sees it in `mine` | ✅ |
| Manager (step 1) sees it in `inbox` | ✅ |
| step-1 reviewer `my_turn = true` | ✅ |
| step-2 reviewer `my_turn = false` (not yet their turn) | ✅ |

### E. Decisions — via the reused workflow engine
| Check | Result |
|---|---|
| out-of-turn decision (step 2 before step 1) → **409** | ✅ |
| non-assigned editor decides → **403** | ✅ |
| viewer decides → **403** (role gate) | ✅ |
| assigned reviewer approves → 200 | ✅ |
| after approval, step-2 reviewer `my_turn = true` | ✅ |
| step-2 reviewer approves → 200 | ✅ |
| request status becomes **approved** | ✅ |

### F. Access control on request detail
| Check | Result |
|---|---|
| uninvolved viewer → **404** (no existence leak) | ✅ |
| uninvolved editor → **404** | ✅ |
| requester → 200 · reviewer → 200 · admin → 200 | ✅ |
| unknown id → 404 | ✅ |

### G. Submitter policy
| Check | Result |
|---|---|
| a **viewer can submit their own request** → 200 | ✅ |

### H. Seed + cleanup
| Check | Result |
|---|---|
| two seeded demo requests present on a fresh boot | ✅ |
| no leftover test anchor files after cleanup | ✅ |

---

## Frontend (verified live in the browser)

- **`/requests`** — "Waiting on you" / "My requests" / "All" tabs with counts,
  status pills (In review / Approved / Your turn), rows opening the detail view.
- **`/requests/new`** — chat-first: the user types plainly → the AI draft card
  fills the form (editable inline) and shows the proposed route with a "why";
  **"Fill a form instead"** drops to the manual form (kind dropdown); attachment
  upload for editors. **Send** posts the request and jumps to detail.
- **`/requests/[id]`** — the AI summary written for the approver, the submitted
  form, the attached document (links to the file view), and the **reused
  `WorkflowPanel`** for approve / reject / send-back (with the panel's redundant
  "Start workflow" form suppressed via a new `hideStart` prop).
- Everyday **nav item** + bilingual **EN/TH** strings; **dark mode** works (all
  colors via tokens). `tsc --noEmit` clean; `next build` compiles all three
  routes.

## Conclusion

The AI-first Requests flow works end to end and rides the proven approval engine
rather than duplicating it. The magic path — *describe it → AI fills the form and
routes it → send → it flows to approvers* — is live, and every guard that
protects the workflow engine (turn order, reviewer identity, role gate,
per-request access) protects requests too, because it **is** the same engine.

**Method note:** the reused engine's decision/turn-guard/send-back paths were
already covered by the earlier workflow + mutation sweeps; this run focuses on
the new request layer (intake, routing policy, create, list boxes, detail
access) and confirms it composes correctly with that engine.
