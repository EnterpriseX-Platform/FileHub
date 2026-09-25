import type { Branding } from "@/lib/branding-shared";

/// Shown when the identity proxy says the user is signed in but has no File Hub
/// role (backend 403).  A blank page or a bounce to the console login would
/// look like an outage; saying it is an access question — and where to ask —
/// saves a round of support calls.  Wording and portal link come from the
/// workspace branding settings.
export function AccessDenied({ email, branding }: { email?: string; branding: Branding }) {
  return (
    <div
      style={{
        minHeight: "100vh",
        display: "grid",
        placeItems: "center",
        padding: 24,
        background: "var(--bg-subtle)",
      }}
    >
      <div className="card" style={{ maxWidth: 520, padding: 28, textAlign: "center" }}>
        <div
          style={{
            width: 48, height: 48, margin: "0 auto 16px",
            borderRadius: 12, display: "grid", placeItems: "center",
            background: "var(--danger-soft)", border: "1px solid var(--danger-border)",
            color: "var(--danger)", fontSize: 22, fontWeight: 700,
          }}
          aria-hidden
        >
          !
        </div>
        <h1 className="t-lg t-semibold" style={{ margin: "0 0 8px" }}>
          You don’t have access to {branding.workspace_display}
        </h1>
        <p className="t-sm t-muted" style={{ margin: "0 0 4px", lineHeight: 1.7 }}>
          You are signed in, but your account has not been granted access to {branding.workspace_display}.
        </p>
        {email && (
          <p className="t-xs t-subtle" style={{ margin: "0 0 16px" }}>
            Signed in as: {email}
          </p>
        )}
        <p className="t-sm t-muted" style={{ margin: "0 0 20px", lineHeight: 1.7 }}>
          {branding.access_help}
        </p>
        {branding.portal_url && (
          <a href={branding.portal_url} className="btn primary" style={{ textDecoration: "none" }}>
            {branding.portal_label}
          </a>
        )}
      </div>
    </div>
  );
}
