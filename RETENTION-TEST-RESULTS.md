# Retention / Rotation Policy — End-to-End Test Results

**Date:** 2026-07-03
**Scope:** Live E2E against the running stack (backend `:8090` + Postgres). Every
lifecycle stage exercised through the real API and rotation engine; day-based
rules were driven by backdating row timestamps in Postgres (they cannot be
waited out in a test window). All destructive checks were isolated in a
throwaway system, and the workspace default policy was neutralised for the
duration and **restored exactly** afterward.

**Result: 21 / 21 checks passed. No failures. Real demo data verified untouched.**

---

## What the retention engine does (`backend/src/rotation.rs`)

The rotation worker (`ROTATION_INTERVAL_SECS`, `0` disables; also triggerable
on demand via `POST /api/rotation/run`) runs three lifecycle actions plus two
housekeeping reaps on each tick:

| Stage | Rule | Effect |
|---|---|---|
| **Version pruning** | keep newest `keep_last_n_versions` (current row + N‑1 history) | old `file_versions` rows + backing blobs evicted |
| **Archive** | live file with `created_at` older than `archive_after_days` | soft‑deleted (`deleted_at = now()`) → moves to Trash, storage not yet freed |
| **Hard delete** | trashed file with `deleted_at` older than `delete_after_days` | row + versions + previews + share‑links purged in one transaction; bytes unlinked after commit |
| Reap TUS | abandoned resumable uploads past `expires_at` | partial blob + row removed |
| Reap check‑outs | locks older than `CHECKOUT_TTL_HOURS` (default 8h) | lock cleared |

**Policy resolution — most‑specific wins:** `user > org > system > workspace`.
Each axis (keep / archive / delete) is resolved independently, so a narrow
policy can shorten one dimension without wiping the workspace baseline.

**Report state mapping (`backend/src/reports.rs`)** — the TOR document‑status
report maps this lifecycle onto four states:

| State | Definition |
|---|---|
| **Active** | `deleted_at IS NULL` |
| **Inactive** | trashed/archived but within the retention window |
| **Retention** | trashed longer than the retention threshold (eligible for purge) |
| **Deleted** | cumulative `files_hard_deleted` from `rotation_runs` history |

---

## Test matrix

### 0. Baseline (for the data-safety proof)
`active files = 22 · contract‑A12 versions = 6 · workspace policy (keep/arch/del) = 5/0/30`

### 1. Version pruning — and system-scope overrides workspace
- Uploaded a file into the test system, added 4 historical versions.
- Set a **system** policy `keep=2` while the **workspace** policy was neutralised to `keep=9999`.
- Ran rotation.

| Check | Want | Got |
|---|---|---|
| historical versions pruned to 1 (system beat workspace 9999) | 1 | 1 ✅ |
| run reported `versions_pruned` | 3 | 3 ✅ |

### 2. Policy resolution — user scope overrides system scope
- Same test system, a versioned file owned by admin; added a **user** policy `keep=1` over the system's `keep=2`.

| Check | Want | Got |
|---|---|---|
| user `keep=1` overrode system `keep=2` (0 historical kept) | 0 | 0 ✅ |

### 3. Archive — live file older than `archive_after_days`
- Backdated the file's `created_at` to 40 days ago; set system `archive_after_days=30`; ran.

| Check | Want | Got |
|---|---|---|
| 40‑day‑old file archived (`deleted_at` set) | true | true ✅ |
| run reported `files_archived ≥ 1` | yes | yes ✅ |
| archived file now appears in Trash | 1 | 1 ✅ |

### 4. Hard delete — trashed file older than `delete_after_days`
- Backdated `deleted_at` to 40 days ago; set `delete_after_days=30`; ran.

| Check | Want | Got |
|---|---|---|
| file row purged from DB | 0 | 0 ✅ |
| its version rows purged too | 0 | 0 ✅ |
| run reported `files_hard_deleted ≥ 1` | yes | yes ✅ |
| **audit/activity rows survive** the hard delete (FK `ON DELETE SET NULL`) | ≥1 | ≥1 ✅ |

### 5. Concurrent-restore safety
The hard-delete transaction re-checks the delete window inside the `DELETE`, so a
restore that lands after the pre-check still wins the race.
- Trashed + backdated a file, **restored** it, then ran rotation.

| Check | Want | Got |
|---|---|---|
| restored file survives the run (not purged) | 1 | 1 ✅ |

### 6. RBAC on rotation endpoints

| Check | Want | Got |
|---|---|---|
| editor can read policies | 200 | 200 ✅ |
| editor can read run history | 200 | 200 ✅ |
| editor cannot upsert a policy | 403 | 403 ✅ |
| editor cannot trigger a run | 403 | 403 ✅ |
| viewer cannot read policies | 403 | 403 ✅ |
| viewer cannot trigger a run | 403 | 403 ✅ |

Policy writes and manual runs are **admin-only**; reads are admin+editor; viewers
are shut out entirely.

### 7. Report lifecycle mapping
Snapshot during the run: `active=23 · inactive=69 · retention=0 · deleted=1`.
- `retention=0` is correct — all real trashed files were deleted recently
  (within the window → *inactive*), not yet aged into *retention*.
- `deleted=1` is the file this run hard-deleted, counted from rotation-run
  history rather than a surviving row — the audit path working as designed.
- (`active=23` was the transient count with the test file still present; back to
  22 after cleanup.)

### 8. Data safety — real data restored exactly

| Check | Want | Got |
|---|---|---|
| workspace policy restored to 5/0/30 | 5/0/30 | 5/0/30 ✅ |
| exactly one workspace policy row (no duplicate) | 1 | 1 ✅ |
| real active file count unchanged | 22 | 22 ✅ |
| contract‑A12 versions untouched | 6 | 6 ✅ |

---

## Conclusion

The full retention lifecycle — **version pruning → archive → hard delete** — works
correctly end to end, with policy resolution honouring the documented
`user > org > system > workspace` precedence, admin-only enforcement on writes and
manual runs, audit history surviving permanent deletion, and a concurrent-restore
guard that protects a file a user rescues at the last moment. The rotation engine
is idempotent (safe for both cron and manual triggers) and every run is logged to
`rotation_runs` with per-stage counts, which is also what feeds the TOR
document-status report's *Deleted* state.

**Method note:** archive/delete are day-based, so their triggers were reproduced
by backdating `created_at` / `deleted_at` in Postgres rather than waiting.
Everything else — policy CRUD, resolution, pruning, runs, RBAC, report mapping —
ran unmodified through the live API. No production code paths were stubbed.
