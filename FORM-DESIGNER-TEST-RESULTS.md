# Form Designer + Admin-Authored Forms — E2E Results

**Date:** 2026-07-03
**What changed:** The Requests feature's four form types and their routing were
**hardcoded in Rust**. They are now **admin-authored data** — an agency defines
its own request forms (fields + the approval route each follows) in the Admin
Console, with no code change or redeploy. The Everyday flow and the AI intake
read these forms live.

**Result: 19 / 19 Form Designer checks + 31 / 31 Requests checks passed** (live).

---

## What was built

- **`request_forms` table** (`migration 0023`) — each row is a request type: name
  (EN/TH), icon, color, an ordered set of fields (key, labels, type, required),
  and a link to a `workflow_templates` route.
- **Non-destructive seed** — the four defaults (Expense · IT/Service · Document ·
  Leave) are seeded at startup with `ON CONFLICT DO NOTHING`, each linked to a
  seeded template. **Admin edits survive reboots.**
- **Form Designer** (`/settings/forms`, Admin Console, editor+) — a builder for
  name/icon/color, an add/remove/reorder **field editor** (text · textarea ·
  number · money · date), a **workflow-template picker** for the route, and an
  active toggle. Cross-linked to the existing **Workflow Designer**
  (`/settings/workflows`), which authors the routes.
- **Backend rewire** — `request-forms` + AI intake read DB forms (the model
  classifies into whatever forms the workspace has); `create` resolves the route
  from the form's **linked template**; list/detail carry the form's icon/color/
  label so custom forms render everywhere.
- **Everyday consumes forms dynamically** and **never shows the designers** — the
  form dropdown, field rendering, and default route all come from the DB; the
  approver pool for the requester's edit-step comes from members.

## Decisions honored

- **One workflow template per form** (no automatic amount-conditions — the old
  prototype's ">฿5,000 → add Director" rule was dropped; an admin can instead
  make a dedicated high-value form/route).
- **Admin + editor** can use the designers (matches Admin Console access).
- **Requester can still edit approvers** at submit; the form's route is the default.

---

## Form Designer test matrix (19/19)

### A. Seeded defaults
| Check | Result |
|---|---|
| 4 default forms seeded on boot | ✅ |
| each default links a workflow template | ✅ |
| `request-forms` resolves each form's route | ✅ |

### B. Designer CRUD (editor)
| Check | Result |
|---|---|
| editor creates a custom form → gets an id | ✅ |
| the custom form appears in `request-forms` (Everyday) | ✅ |
| its route resolves from the linked template (Legal → Director) | ✅ |
| editor PATCHes the form → 200 | ✅ |
| setting `active=false` **hides it from Everyday** | ✅ |
| …but it stays visible in the designer | ✅ |

### C. A request via a custom form
| Check | Result |
|---|---|
| request created via the custom form → in_review | ✅ |
| **routes through the form's template** (Legal → Director) | ✅ |
| detail carries the form's icon | ✅ |

### D. RBAC — designers are editor+
| Check | Result |
|---|---|
| viewer list / create / delete forms → **403** | ✅ |
| editor delete form → 204 · delete unknown → 404 | ✅ |

### E. Cleanup
| Check | Result |
|---|---|
| back to the 4 default forms | ✅ |
| back to the 2 seeded demo requests | ✅ |

## Regression — the original Requests flow (31/31)

Re-run after the refactor. Two assertions were updated to the new (intended)
behavior, not product fixes: `request-forms` is now keyed by form **`id`** (was a
`kind` enum), and expense routing is the form's template with **no auto-Director
condition** (dropped by design). Everything else — intake, create, list boxes,
sequential turn-guard, per-request access, viewer-submit — unchanged and green.

---

## Verified live in the browser

- Form Designer lists the 4 forms with their icons; the **builder** (icon + color
  swatches, field rows, template picker) creates a form that immediately appears
  in the list **and in Everyday's `request-forms`**; the UI **delete** restores
  the defaults.
- Everyday **New request → "Fill a form instead"** shows the DB forms in the
  dropdown, renders the selected form's fields, and shows the route from its
  linked template.
- `tsc --noEmit` clean; `next build` compiles `/settings/forms` + all request routes.

## Conclusion

Requests is now **configuration, not code**: an agency authors its own forms and
approval routes entirely in the Admin Console, the AI classifies plain-language
requests into them, and the Everyday app renders them live — while the designers
themselves stay Admin-Console-only. The four defaults ship as editable starting
points, and the whole thing rides the same approval engine as before.
