# Write / Mutation Integrity Sweep — quota · checkout-lock · workflow-turn

**Date:** 2026-07-03
**Scope:** Live E2E confirming the three write-side guards cannot be bypassed
cross-user. Adversarial framing: a second user (admin, the strongest role)
attempts to circumvent a guard set up by another user. Isolated in a throwaway
system; real demo data restored and verified.

**Result: 26 / 26 checks passed.** One real defect was found and fixed: the
check-out lock only protected version uploads, not rename/move/status/delete
(§Fix).

---

## Fix made this sweep

**Check-out lock was incomplete (TOR 5.3.8.4-5).** `lock_blocks` was called
only in `upload_version` and `restore_version`, so a file another user had
**checked out for exclusive editing** could still be **renamed, moved,
re-statused, or trashed/hard-deleted** by anyone else. `patch_file` and
`delete_file` now both enforce `lock_blocks`: the holder is unaffected; any
non-holder — **admin included** — gets `409` and must check in first. That's
deliberate, since `checkin` already lets an admin release another user's lock
as an explicit, **audited** action (an activity row is written), so nothing is
stranded and no lock is silently overridden.

---

## A. Quota caps (additive: workspace → system → org → user)

Every upload path (`persist_upload`, `upload_version`, WOPI `put_file`, TUS
finalise) calls `enforce_quota` **before** writing bytes; each level checks
`used + incoming ≤ cap`, and `used` excludes soft-deleted files.

| Check | Result |
|---|---|
| first upload under the system cap → 200 | ✅ |
| upload that would exceed the cap → 413 | ✅ |
| oversize single upload → 413 | ✅ |
| **cross-user: a different user is blocked by the same shared system cap** → 413 | ✅ |
| trashing a file frees quota — next upload fits → 200 | ✅ |
| **new-version upload that busts the cap → 413** (can't grow a file past cap) | ✅ |
| rejected version left **no orphan** row (checked before the write) | ✅ |

## B. Check-out lock (cross-user)

A file checked out by an editor; the admin (non-holder) attempts every
mutation.

| Check | Result |
|---|---|
| editor checks out the file → 200 | ✅ |
| non-holder new-version upload → 409 | ✅ |
| non-holder restore-version → 409 | ✅ |
| **non-holder rename (patch) → 409** (fixed this sweep) | ✅ |
| **non-holder status change (patch) → 409** (fixed) | ✅ |
| **non-holder trash (delete) → 409** (fixed) | ✅ |
| **non-holder hard-delete → 409** (fixed) | ✅ |
| non-holder second check-out → 409 | ✅ |
| the **holder** can rename freely → 200 | ✅ |
| admin **force check-in** another's lock (audited) → 200 | ✅ |
| after check-in, admin can rename → 200 | ✅ |

## C. Workflow-turn guard (cross-user)

Sequential workflow, step 1 = admin, step 2 = editor.

| Check | Result |
|---|---|
| step-2 reviewer decides **out of turn** → 409 | ✅ |
| a **non-assigned** editor decides step 1 → 403 | ✅ |
| a viewer decides step 1 → 403 (role gate) | ✅ |
| the **assigned** reviewer decides step 1 → 200 | ✅ |
| now the step-2 reviewer can decide → 200 | ✅ |
| **parallel** workflow: a reviewer decides without waiting → 200 | ✅ |

## Cleanup
| Check | Result |
|---|---|
| real active file count restored to 22 | ✅ |
| no leftover test system | ✅ |

---

## Conclusion

All three write-side guards are sound cross-user. Quota is enforced additively
before any bytes hit storage and can't be dodged by switching users or routing
through the version path. The workflow engine enforces both *who* (only the
assigned reviewer) and *when* (sequential order), while parallel mode correctly
lifts the ordering. The one real gap — the check-out lock protecting only
version uploads — is closed: **rename, move, re-status, trash, and hard-delete
now all respect the lock**, with an audited admin force-check-in as the only
way to break it. Combined with the earlier read-surface sweeps, both the read
and write sides of the collaboration model have now been adversarially tested.

**Method note:** the admin was used as the "attacker" precisely because it's
the highest-privilege role — if even an admin can't bypass a lock without an
explicit, logged check-in, no lesser role can. All quota levels were driven by
setting `systems.quota_bytes` directly; every other path ran through the live
API.
