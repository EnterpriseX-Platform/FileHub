# Version Control — End-to-End Test Results

**Date:** 2026-07-03
**Scope:** Live E2E against the running stack (backend `:8090` + Postgres),
plus a browser check of the version-history UI. Each version used **distinct
known content** so integrity (`etag = MD5 of plaintext`) and non-destructive
restore could be proven **byte-for-byte** via md5 comparison. All API checks
ran through a dedicated throwaway file, hard-deleted at the end.

**Result: 34 / 34 API checks passed + UI restore verified in-browser. No failures.**

---

## Model under test (`file_versions`, TOR 4.15.7)

- The **current** version lives on the `files` row (`files.version`,
  `object_key`, `etag`). Historical snapshots live in `file_versions`
  (unique on `(file_id, version)`, `ON DELETE CASCADE`).
- **Upload version** — snapshots the current bytes into `file_versions` under
  the old version number, then bumps `files.version` and points it at the new
  blob. Re-indexes thumbnail / preview / search / AI against the new bytes.
- **Restore** (`POST /files/:id/versions/:v/restore`) — **non-destructive**:
  snapshots the current bytes as a new history row, points the file at the
  chosen version's blob, and bumps to a *fresh* version number (never resets).
- **Checkout lock** (`checkout::lock_blocks`) gates both upload-version and
  restore: a file checked out by someone else can't be revised by anyone but
  the holder.
- **ETag** is the MD5 of the *plaintext*, so it's stable across storage
  re-keying and changes only when content changes.

---

## Test matrix

### 1. Fresh file starts at v1, empty history
| Check | Want | Got |
|---|---|---|
| new file `version = 1` | 1 | 1 ✅ |
| current `etag = md5(v1)` | ✓ | ✓ ✅ |
| history rows = 0 (current **is** v1) | 0 | 0 ✅ |

### 2. Upload v2 → bump + snapshot the old v1
| Check | Want | Got |
|---|---|---|
| file version now 2 | 2 | 2 ✅ |
| current `etag = md5(v2)` | ✓ | ✓ ✅ |
| history rows = 1 | 1 | 1 ✅ |
| snapshot is `v1` with `etag = md5(v1)` | ✓ | ✓ ✅ |
| snapshot captured the `note` | "second revision" | ✓ ✅ |

### 3. Upload v3, v4 → chain accumulates
| Check | Want | Got |
|---|---|---|
| file version now 4 | 4 | 4 ✅ |
| history rows = 3 (v1,v2,v3) | 3 | 3 ✅ |
| ordered newest-first | 3,2,1 | 3,2,1 ✅ |
| every version's etag distinct | ✓ | ✓ ✅ |

### 4. Download serves the current (v4) content
| Check | Want | Got |
|---|---|---|
| downloaded bytes == v4 (md5) | ✓ | ✓ ✅ |

### 5. Non-destructive restore of v1 — the core scenario
| Check | Want | Got |
|---|---|---|
| restore bumped to **v5** (not reset to 1) | 5 | 5 ✅ |
| current content now == **v1 bytes** (md5) | ✓ | ✓ ✅ |
| current etag now == md5(v1) | ✓ | ✓ ✅ |
| history preserved: v1..v4 = 4 rows | 4 | 4 ✅ |
| the superseded v4 snapshotted into history | 4,3,2,1 | 4,3,2,1 ✅ |

### 6. Restore edge case
| Check | Want | Got |
|---|---|---|
| restore a non-existent version → 404 | 404 | 404 ✅ |

### 7. Version numbers unique + monotonic
| Check | Want | Got |
|---|---|---|
| no duplicate version numbers | 0 | 0 ✅ |
| max history version = 4 (current is 5) | 4 | 4 ✅ |

### 8. Checkout-lock interaction
| Check | Want | Got |
|---|---|---|
| editor can check out | 200 | 200 ✅ |
| **non-holder** (admin) version upload blocked | 409 | 409 ✅ |
| **non-holder** (admin) restore blocked | 409 | 409 ✅ |
| **holder** (editor) can upload a version | 200 | 200 ✅ |
| after check-in, admin version upload works | 200 | 200 ✅ |

### 9. RBAC on versioning
| Check | Want | Got |
|---|---|---|
| viewer can **list** versions (read) | 200 | 200 ✅ |
| viewer cannot upload a version | 403 | 403 ✅ |
| viewer cannot restore | 403 | 403 ✅ |

### 10. Activity trail
| Check | Want | Got |
|---|---|---|
| version-upload activity rows logged (≥5) | yes | yes ✅ |
| restore activity row logged (≥1) | yes | yes ✅ |

### 11. Trashed-file handling
| Check | Want | Got |
|---|---|---|
| list versions on a trashed file → 404 | 404 | 404 ✅ |

### 12. Cleanup integrity
| Check | Want | Got |
|---|---|---|
| hard delete removed the file row | 0 | 0 ✅ |
| hard delete cascaded `file_versions` (6 → 0) | 0 | 0 ✅ |

### UI layer (browser, admin on contract-A12.pdf)
- Version history panel lists every version with number, note, size, and time.
- Six restore controls present (`aria-label="Restore version N"`).
- Clicked **Restore version 3** → toast "Version 3 restored as the current
  version", header bumped **v7 → v8** (non-destructive), Modified "just now",
  and a new snapshot appeared in the list.

---

## Conclusion

Version control is correct end to end. New uploads bump the version and
snapshot the prior content with its uploader, note, size, and MD5 etag;
restore is genuinely non-destructive (it advances the version and preserves
the full chain, verified byte-for-byte); the checkout lock correctly gates
both revise paths to the holder; RBAC keeps viewers read-only; every event is
audited; and hard delete cascades the history cleanly. The version-history UI
and one-click restore work in the browser with a confirming toast.

**Method note:** distinct known payloads let integrity and restore be proven
by MD5 rather than assertion. Real demo data was verified intact afterward
(22 active files, throwaway test file cascaded away). Note contract-A12.pdf is
now at v8 — the deliberate UI restore above added one non-destructive version
to the showcase file, which is expected.
