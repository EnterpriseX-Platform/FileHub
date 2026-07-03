# Share-Link Security Sweep — token scoping · expiry · revocation · minimal leak

**Date:** 2026-07-03
**Scope:** The deliberately-*public* surface — tokenized share links. Adversarial
framing: an anonymous holder of a link (no session at all) is the attacker, plus
a viewer and a non-creating editor probing the management side. What can a link
reach, for how long, can it be killed, and does the anonymous payload leak more
than a download page needs? Isolated in a throwaway system; real demo data
restored and verified.

**Result: 43 / 43 checks passed.** Two real gaps were found and closed
(§Fixes) before the sweep went green.

---

## Fixes made this sweep

1. **`share_meta` over-exposed the whole file row to anonymous recipients.**
   `GET /api/share/{token}` returned the full `File` struct — `bucket`,
   `object_key`, `system_id`, `org_id`, `owner`, `created_by`, `etag`,
   `encrypted`, `folder_id`, `version`, internal `id`. A link meant to let a
   vendor download one contract also handed them the storage layout and the org
   structure behind it. Now it returns a minimal, purpose-built `ShareMeta` DTO
   — **only `name`, `file_type`, `size_bytes`, `created_at`**, the four fields a
   download page actually renders. The bytes still come from
   `share/{token}/download`; nothing about internal storage or org topology
   leaves the boundary.

2. **A share link could not be revoked.** There was no list or delete endpoint —
   once created, a link lived until it expired (up to 90 days) or the file was
   deleted. Added two owner-side controls (TOR 4.15.11):
   - `GET /api/files/{id}/share-links` — editor+, access-gated, so an owner can
     **see** every link on a file (with an `expired` flag) to decide what to kill.
   - `DELETE /api/share-links/{id}` — **the link's creator or an admin** revokes
     it immediately; the token 404s from that instant. The revocation writes an
     **audit row**, so kills are traceable.

Both fixes are reflected in `openapi.yaml`.

---

## Test matrix

### A. Create gates
| Check | Result |
|---|---|
| editor can create a share link → 200 | ✅ |
| viewer cannot create a share link → 403 (role gate) | ✅ |
| external-sharing toggle **off** blocks creation → 403 | ✅ |
| token is CSPRNG, ≥ 24 chars (24 random bytes → 32 url-safe) | ✅ |
| a 9999-day request is **clamped to ≤ 90 days** (not honored) | ✅ |

### B. Anonymous access + token scoping
| Check | Result |
|---|---|
| anon meta with a valid token → 200 | ✅ |
| anon download with a valid token → 200 | ✅ |
| garbage token: meta → 404, download → 404 | ✅ |
| **token resolves to its own file only** (F1's token yields F1's name) | ✅ |
| download serves **F1's bytes**, `Content-Disposition` names F1, not F2 | ✅ |

### C. Minimal leak — `share_meta` must not carry internals
| Field | Present in anon payload? |
|---|---|
| `object_key`, `bucket` | ❌ omitted ✅ |
| `system_id`, `org_id` | ❌ omitted ✅ |
| `owner`, `created_by`, `created_by_id` | ❌ omitted ✅ |
| `etag`, `encrypted`, `folder_id`, internal `id` | ❌ omitted ✅ |
| `name`, `file_type`, `size_bytes`, `created_at` | ✅ present (the safe four) |

### D. Expiry
| Check | Result |
|---|---|
| fresh token works → 200 | ✅ |
| once past `expires_at`: meta → 404, download → 404 | ✅ |

### E. Revocation
| Check | Result |
|---|---|
| link works before revoke → 200 | ✅ |
| owner lists the file's links (finds the id) → 200 | ✅ |
| **viewer cannot list** a file's share links → 403 | ✅ |
| **non-creator editor cannot revoke** someone else's link → 403 | ✅ |
| creator (admin) revokes → 204 | ✅ |
| revoked token: meta → 404, download → 404 (dead immediately) | ✅ |
| revoke an unknown id → 404 | ✅ |
| revocation wrote an **audit row** | ✅ |

### F. Soft-delete kills the link
| Check | Result |
|---|---|
| link works while the file is active → 200 | ✅ |
| after the file is trashed: meta → 404, download → 404 | ✅ |

### Cleanup
| Check | Result |
|---|---|
| real active file count restored to 22 | ✅ |
| no leftover ShareLink Test system | ✅ |

---

## Conclusion

The share-link surface is sound on all four axes the request named:

- **Scoping** — a token maps to exactly one file. There is no id arithmetic, no
  enumeration (24-byte CSPRNG tokens), and a token never surfaces a neighbouring
  file's name or bytes.
- **Expiry** — TTL is mandatory, defaults to 7 days, and is hard-clamped to 90
  even when the caller asks for 9999. `share_target` re-checks `expires_at` on
  every anonymous hit, so an expired link 404s for both meta and download.
- **Revocation** — now possible and immediate: the creator or an admin deletes
  the link and the token dies the same instant, with an audit trail. Soft-
  deleting the file has the same killing effect.
- **Minimal leak** — the anonymous meta payload was trimmed from the entire file
  row to four display fields, so a link no longer discloses storage keys, bucket
  names, or the org/system structure behind the document.

Two real defects (full-row exposure; no revocation path) were closed; the
CSPRNG token, mandatory-and-clamped TTL, expiry enforcement, and soft-delete
kill-switch were already correct and are now proven by test.

**Method note:** the anonymous caller was used as the primary attacker because a
share link is the one endpoint that intentionally answers with **no session** —
if it holds there, the authenticated surfaces (already swept) are strictly
tighter. The external-sharing toggle, role gate, and creator-or-admin revoke
rule were each exercised from the wrong role to confirm they bounce.
