# Search · Audit Log · WOPI/Collabora — End-to-End Test Results

**Date:** 2026-07-03
**Scope:** Live E2E against the running stack (backend `:8090` + Postgres +
local Ollama for embeddings). Search used distinctive nonce tokens so matches
are exact; WOPI drove FileHub's host endpoints directly (exactly what Collabora
calls), so the office-editing path is verified even without the Collabora
container running. Every check isolated its data and restored the demo state.

**Result: 63 / 63 checks passed.** One real information-disclosure bug was
found in the audit feed and **fixed** (see §2).

| Area | Checks | Result |
|---|---|---|
| Full-text + semantic search | 22 | ✅ all pass |
| Audit log | 15 | ✅ all pass (after fix) |
| WOPI / Collabora office-editing | 26 | ✅ all pass |

---

## 1. Full-text + semantic search (TOR 4.15.14)

Keyword search (`GET /api/search`) runs Postgres FTS (`websearch_to_tsquery`,
`simple` config) over name/project/owner/status/type/tags, plus ILIKE
substring fallbacks (for Thai) and a subquery over extracted document bodies
(`file_content.content_tsv`). Semantic search (`POST /api/search/semantic`) is
pgvector kNN over embedded chunks. Both scope by `effective_system_ids`.

| Check | Result |
|---|---|
| match by file **name** | ✅ |
| match by **tag** | ✅ |
| match by **extracted body** (indexed on upload) | ✅ |
| boolean **AND** (both terms required) | ✅ |
| boolean **AND** correctly excludes when a term is absent | ✅ |
| **phrase** (quoted adjacency) | ✅ |
| **negation** (`term -other`) | ✅ |
| filter by `file_type` (and wrong type excludes) | ✅ |
| filter by `system_id` (and other system excludes) | ✅ |
| **permission scoping** — owner + admin find a personal-drive term, **viewer cannot** | ✅ |
| trashed files excluded from results | ✅ |
| empty query → 400 | ✅ |
| unauthenticated → 401 | ✅ |
| **semantic** returns ranked hits (e.g. "DMS project budget" → meeting-minutes 0.68, TOR-summary 0.66, budget-memo 0.62) | ✅ |
| semantic empty query → 400 · unauth → 401 | ✅ |

*(Note: two initial "failures" were a flawed decoy corpus in the test — the
word under test lived in the body file by accident — not an engine fault.
Corrected corpus → AND/negation both correct.)*

---

## 2. Audit log — **bug found and fixed**

### The finding
`GET /api/activity` (the JSON feed behind the Activity page and Settings ›
Audit) had **no permission scoping** — it didn't even take the caller into
account — so any authenticated user, including a **viewer**, could read the
entire activity feed across every system and **every user's personal drive**
(file names, who did what, when). The CSV export (`audit_csv`) was already
scoped; only the JSON endpoint was not.

### The fix
`list_activity` now resolves `effective_system_ids(user)` and filters
`system_id IS NULL OR system_id = ANY(scope)` — identical to the CSV path.
Admins resolve to `None` → they still see the whole feed (verified: 200 events
unchanged); non-admins see only their systems. System-less rows (account
events) stay visible to everyone.

### Verification (post-fix)

| Check | Result |
|---|---|
| a mutation writes an audit row (actor + action + target) | ✅ |
| feed ordered newest-first | ✅ |
| pagination (`limit` / `offset`) | ✅ |
| CSV export has UTF-8 **BOM** (Excel/Thai) | ✅ |
| CSV header = `timestamp,user,action,target,system` + data rows | ✅ |
| **viewer JSON feed excludes another's personal-drive activity** (was leaking) | ✅ |
| viewer CSV excludes it (already scoped) | ✅ |
| admin JSON feed **does** include it (sees all) | ✅ |
| audit rows **survive hard delete** (`target` kept, `file_id` set NULL) | ✅ |
| unauthenticated JSON / CSV → 401 | ✅ |
| viewer can read own-scope feed → 200 | ✅ |

---

## 3. WOPI / Collabora office-editing path

FileHub is the **WOPI host**: it hands Collabora an iframe URL + a short-lived
HMAC-SHA256 access token, and Collabora calls three endpoints —
`CheckFileInfo`, `GetFile`, `PutFile`. Tested the host directly (which is what
Collabora does); no Collabora container required.

| Check | Result |
|---|---|
| `office-url` default viewer = **pdf**, empty iframe_url | ✅ |
| `office-url` on a non-office file → 400 | ✅ |
| switch to **collabora** → viewer=collabora, iframe has `WOPISrc` + `access_token` | ✅ |
| admin gets **edit** mode; viewer role gets **view** mode | ✅ |
| **CheckFileInfo**: BaseFileName, Size (= actual bytes), Version correct | ✅ |
| edit token → `UserCanWrite=true`, `ReadOnly=false` | ✅ |
| view token → `UserCanWrite=false`, `ReadOnly=true` | ✅ |
| **GetFile** returns the current bytes | ✅ |
| **PutFile** (edit token) → 200, **bumps version**, archives prior version | ✅ |
| current bytes are the edited content; **preview cache invalidated** | ✅ |
| **tampered token → 401** | ✅ |
| **missing token → 400/401** | ✅ |
| **token for file A used on file B → 401** | ✅ |
| **view token cannot PutFile** (save-back blocked) → 401 | ✅ |
| config restored to `pdf`; demo data intact | ✅ |

The save-back path is the important one: a Collabora edit lands as a **new
file version** (history preserved), quota is enforced like a normal upload,
and the cached PDF preview is dropped so the next view rebuilds from the edited
bytes.

---

## Conclusion

Search, audit, and the WOPI office-editing path all work correctly end to end.
The one substantive issue — the unscoped activity JSON feed leaking
cross-system and personal-drive history to any signed-in user — was found by
the audit test and fixed so the JSON feed enforces the same
`effective_system_ids` scope as the CSV export. Method note: search integrity
was proven with exact nonce tokens; WOPI was driven through the real host
endpoints and HMAC tokens minted by `office-url`; all test data was isolated
and the demo restored (22 active files, `office_viewer` back to `pdf`).
