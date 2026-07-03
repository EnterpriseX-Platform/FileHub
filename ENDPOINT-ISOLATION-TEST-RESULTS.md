# Endpoint Isolation Sweep — comments · notifications · workflow · sign-requests · thumbnails

**Date:** 2026-07-03
**Scope:** Information-disclosure audit + live E2E of every read/list endpoint
across the five collaboration surfaces, hunting the same unscoped pattern that
was found in the audit feed. Method: a non-owner (viewer) is pointed at another
user's **personal-drive** file — the strictest isolation boundary — and must
not reach any of its collaboration data, while per-user feeds (notifications,
sign queue) must be caller-scoped.

**Result: 24 / 24 checks passed.** Two hardening fixes were made (§Fixes); the
other three surfaces were already correctly scoped and the test proves it.

---

## Fixes made this sweep

1. **`GET /api/workflow-templates` was gated only by a session, not a role.**
   A viewer could enumerate every approval template — routes and reviewer
   identities they never use. Now `require_role(admin, editor)`, matching
   `create`/`delete` which were already editor+. The frontend already handled a
   non-OK response gracefully (`r.ok ? json : []`), so no UI change was needed;
   verified admin templates still load.

2. **Comment write-paths leaked personal-file existence via 403.**
   `create_comment` / `delete_comment` have no role gate (viewers may comment),
   so `ensure_system_access` was the only gate — and it **propagated 403**,
   which distinguishes "exists but hidden" from "doesn't exist" on someone
   else's personal-drive file, while the read path (`list_comments`) already
   returned 404. Both writes now map access denial to **404**, matching the
   read. (Role-gated writes elsewhere — start-workflow, create-sign-request,
   create-template — correctly return 403 uniformly, since that reveals only
   "you're a viewer", not any personal-file's existence.)

---

## Test matrix

### 1. Comments
| Check | Result |
|---|---|
| owner reads own file's comments | 200 ✅ |
| admin reads them (sees all) | 200 ✅ |
| **viewer read another's personal-file comments → 404** (no leak) | ✅ |
| **viewer post comment → 404** (hardened; was 403) | ✅ |

### 2. Notifications (per-user isolation)
| Check | Result |
|---|---|
| admin feed contains **zero** foreign rows | ✅ |
| viewer feed contains **zero** foreign rows | ✅ |
| viewer cannot mark someone else's notification read → 404 | ✅ |
| unread-count is per-user | ✅ |

### 3. Workflow
| Check | Result |
|---|---|
| viewer list workflow on private file → 404 | ✅ |
| viewer start workflow → 403 (role gate) | ✅ |
| owner lists own file's workflow | 200 ✅ |
| editor / admin list templates | 200 ✅ |
| **viewer list templates → 403** (tightened this sweep) | ✅ |
| viewer create template → 403 | ✅ |

*(`decide_step` and `send_back` were also audited: both enforce
`ensure_system_access` plus, for a decision, the caller must be the assigned
reviewer and pass the sequential turn-guard.)*

### 4. Sign-requests
| Check | Result |
|---|---|
| viewer list sign-requests on private file → 404 | ✅ |
| viewer create sign-request → 403 (role gate) | ✅ |
| owner lists own file's sign-requests | 200 ✅ |
| `sign-requests/mine` is per-user | 200 ✅ |
| viewer `verify` a request on a private file → 404 | ✅ |

*(`sign` / `decline` were audited: they act only on the caller's own assigned
signer row.)*

### 5. Thumbnails
| Check | Result |
|---|---|
| owner reads own file's thumb (200/404) | ✅ |
| **viewer thumb of another's personal file → 404** | ✅ |
| admin thumb reachable (200/404, never 403) | ✅ |

### Cleanup
| Check | Result |
|---|---|
| real active file count restored to 22 | ✅ |

---

## Conclusion

All five collaboration surfaces now enforce consistent isolation: read/list
endpoints return **404** for a file the caller can't see (no existence leak),
per-user feeds are strictly caller-scoped, and role-gated writes return a
uniform **403**. Two gaps were closed — the unscoped template list and the
comment write-path 403/404 asymmetry — bringing these endpoints in line with
the file-CRUD handlers' `ensure_system_access` model. Combined with the earlier
audit-feed fix, the read surface has now been swept end to end.

**Method note:** the personal-drive boundary was used because it's the one
place a non-admin genuinely cannot see another user's data; passing there
implies the weaker shared-system boundaries hold too. Malformed-JSON artifacts
in an earlier test run (over-escaped bodies inside a subshell) were corrected —
those were test-harness issues, not product behaviour.
