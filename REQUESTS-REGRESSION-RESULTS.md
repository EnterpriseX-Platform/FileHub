# Requests System — Regression · Security · UI · CI Sweep

**Date:** 2026-07-04
**Scope:** A four-campaign sweep of the whole Requests system after the build-out
(Form/Workflow Designers, intake, notifications, activity trail, select fields,
modals, TH). **One real bug was found and fixed** (§Fix); everything else passed.

- **Functional regression + edge cases + security:** 29 / 29 (live curl)
- **Cross-shell UI** (dark mode · mobile · TH): verified with screenshots
- **CI parity:** `next build` compiles clean; backend `cargo build` clean

---

## Fix made this sweep

**A request title with a Windows-reserved character 500'd.** `sanitize_filename`
stripped path separators + control chars but **not** the other characters
reserved on Windows filesystems (`< > : " | ? *`). A request whose title
contained any of them (e.g. `Report <draft>`) generated an object key the `fs`
backend couldn't create — `fs::put` failed with `os error 123` and the create
POST returned **500**. The full reserved set is now stripped; the display name in
the DB is untouched (only the on-disk key is sanitized). Found by the injection
edge-case test; verified a title with `<script>…</script> | "q" : end` now
creates cleanly and stores its title verbatim. (`fix(storage)`)

---

## 1. Functional regression + edge cases + security (29/29)

### Functional — decision paths
| Check | Result |
|---|---|
| manager **rejects** step 1 → request status `rejected`, note in the trail | ✅ |
| **send-back** re-opens the step (→ pending), request back to `in_review`, reviewer re-notified — **with the `/requests/:id` link** | ✅ |

### Regression — the shared engine still serves file workflows
| Check | Result |
|---|---|
| a workflow on a **regular file** (not a request) still notifies with a **`/f/:id`** link (the `subject_link` change didn't break file workflows) | ✅ |

### Edge cases
| Check | Result |
|---|---|
| **delete a form that has a live request** → the request still loads (icon falls back to `generic`) | ✅ |
| **inactive form** → hidden from the everyday `request-forms`, but an existing request of that kind still labels correctly | ✅ |
| **editing a template** leaves **in-flight** workflows untouched (the request keeps its original steps) | ✅ |

### Security — RBAC / IDOR / injection
| Check | Result |
|---|---|
| viewer list / create / patch / delete `/api/forms` → **403** | ✅ |
| uninvolved viewer / editor GET a request → **404** (no existence leak) | ✅ |
| non-assigned editor decides a step → **403**; viewer decides → **403** | ✅ |
| `DROP TABLE` in `form_data` / title / form name → **tables intact** (parameterized), value **stored verbatim** as JSONB (not executed) | ✅ |
| reserved-char + injection title → **creates cleanly** (the fix above) | ✅ |

*(By design, a requester may edit the approver route before sending — so an
editor could route a request to themselves. That is the chosen product behavior,
not a defect; approvals remain gated to editor+.)*

## 2. Cross-shell UI (dark mode · mobile · TH)

| Surface | Result |
|---|---|
| Request detail (AI summary, form, attached doc, **activity trail**, workflow) in **dark mode** | ✅ readable, tokens correct |
| **Form Designer modal** (icon/color swatches, fields, route picker) in **dark mode** | ✅ proper scrim + contrast |
| Request detail at **375px mobile** — two-column grid collapses to one, content stacks | ✅ |
| Everyday request flow in **Thai** — title `คำร้อง`, tabs `รอคุณตัดสินใจ / คำร้องของฉัน / ทั้งหมด` | ✅ |
| Both **designers in Thai** — `ฟอร์มใหม่ · ไอคอน · สี · ช่องกรอก · เส้นทางอนุมัติ · สร้างฟอร์ม` | ✅ |

## 3. CI parity

| Check | Result |
|---|---|
| `cargo build` (backend) | ✅ clean |
| `next build` (frontend) — `/requests`, `/requests/[id]`, `/requests/new`, `/settings/forms` all compile | ✅ |

---

## Conclusion

The Requests system holds up under an adversarial + edge sweep. The decision
paths (reject, send-back) behave correctly and update both the status and the
activity trail; the shared workflow engine still serves regular file workflows
after the notification-link change; forms can be deleted, deactivated, and their
templates edited without breaking live requests; and the new endpoints enforce
the same role/access model as the rest of the app. The one real defect — a
filename-sanitization gap that 500'd on Windows-reserved characters — is closed
and cross-platform-safe. Dark mode, mobile, and Thai all render correctly across
the new screens and both designers, and the whole thing builds for production.
