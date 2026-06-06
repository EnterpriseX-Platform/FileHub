/// Frontend mirror of the backend `require_role(&["admin", "editor"])` gate
/// (see backend/src/auth.rs + the matrix in app/settings/roles/page.tsx).
///
/// The Rust backend is the real enforcement — it returns 403 for viewers on
/// every mutating route regardless of what the UI shows.  These helpers only
/// decide whether to *render* a control, so a viewer isn't handed buttons that
/// will bounce with a 403.  Keep them in sync with the capability matrix.
///
/// `role` is intentionally permissive about its input (string | null |
/// undefined) so callers can pass the still-loading client auth state or the
/// server `loadServerCtx().role` directly.  Anything that isn't a known
/// mutating role (including null/loading) fails closed → control hidden.

/// True for admin + editor: may upload, edit, delete, restore, share, and
/// create folders.  Viewers and unknown/loading roles → false.
export function canMutate(role: string | null | undefined): boolean {
  return role === "admin" || role === "editor";
}

/// True only for admins: hard-delete (purge) from Trash, bucket/member/quota
/// administration.  Mirrors `require_role(&["admin"])`.
export function isAdmin(role: string | null | undefined): boolean {
  return role === "admin";
}
