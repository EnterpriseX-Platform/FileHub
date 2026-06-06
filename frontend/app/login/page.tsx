"use client";

import { useRouter, useSearchParams } from "next/navigation";
import * as React from "react";

import { useAuth } from "@/lib/auth-context";

export const dynamic = "force-dynamic";

const DEMO_ACCOUNTS = [
  ["admin@acme.go.th",  "admin123",  "Admin role: can hard-delete + full RBAC"],
  ["anong@acme.go.th",  "anong123",  "Editor role: upload, edit, share"],
  ["viewer@acme.go.th", "viewer123", "Viewer role: read-only"],
];

// The demo-account panel (plaintext passwords + autofill + email prefill) is a
// DEV convenience — it makes the login look like a mockup in front of real
// users.  Default: shown only outside production builds.  Override either way
// with NEXT_PUBLIC_SHOW_DEMO_ACCOUNTS=1/0 (e.g. to enable on a demo deploy).
const SHOW_DEMO_ACCOUNTS =
  (process.env.NEXT_PUBLIC_SHOW_DEMO_ACCOUNTS ??
    (process.env.NODE_ENV !== "production" ? "1" : "0")) === "1";

export default function LoginPage() {
  return (
    <React.Suspense fallback={null}>
      <LoginInner />
    </React.Suspense>
  );
}

function LoginInner() {
  const router  = useRouter();
  const params  = useSearchParams();
  const { reload, user, loading } = useAuth();
  const [email, setEmail]       = React.useState(SHOW_DEMO_ACCOUNTS ? "anong@acme.go.th" : "");
  const [password, setPassword] = React.useState("");
  const [busy, setBusy]         = React.useState(false);
  const [error, setError]       = React.useState<string>("");

  // Already logged in?  Bounce straight to the redirect target.
  React.useEffect(() => {
    if (!loading && user) {
      router.replace(params.get("next") ?? "/");
    }
  }, [loading, user, router, params]);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const r = await fetch("/filehub/api/auth/login", {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email, password }),
      });
      if (r.status === 401) {
        setError("Email or password is wrong");
        return;
      }
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      reload();
      router.replace(params.get("next") ?? "/");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{
      minHeight: "100vh", display: "flex", alignItems: "center", justifyContent: "center",
      background: "var(--bg-subtle)", padding: 24,
    }}>
      <div style={{
        display: "grid",
        gridTemplateColumns: SHOW_DEMO_ACCOUNTS ? "1fr 1fr" : "1fr",
        gap: 24,
        maxWidth: SHOW_DEMO_ACCOUNTS ? 880 : 400,
        width: "100%",
      }}>
        <form onSubmit={submit} className="card" style={{ padding: 28, display: "flex", flexDirection: "column", gap: 14 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span style={{ width: 32, height: 32, borderRadius: 8, background: "#4f46e5", color: "#fff", display: "flex", alignItems: "center", justifyContent: "center", fontWeight: 700 }}>F</span>
            <div>
              <div className="t-xl t-semibold">File Hub</div>
              <div className="t-xs t-muted">acme.go.th · Digital Content Platform</div>
            </div>
          </div>

          <div>
            <div className="t-xs t-subtle t-medium" style={{ marginBottom: 4 }}>Email</div>
            <div className="field" style={{ width: "100%" }}>
              <input
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="you@acme.go.th"
                style={{ width: "100%" }}
                autoFocus
                required
              />
            </div>
          </div>

          <div>
            <div className="t-xs t-subtle t-medium" style={{ marginBottom: 4 }}>Password</div>
            <div className="field" style={{ width: "100%" }}>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="••••••"
                style={{ width: "100%" }}
                required
              />
            </div>
          </div>

          {error && (
            <div className="t-sm" style={{ color: "var(--danger)", marginTop: -4 }}>{error}</div>
          )}

          <button type="submit" className="btn primary" disabled={busy} style={{ justifyContent: "center", padding: "10px 14px" }}>
            {busy ? "Signing in…" : "Sign in"}
          </button>

          <div className="t-xs t-subtle">
            Sessions are HttpOnly cookies and expire after 14 days. Encryption at rest uses AES-256-GCM.
          </div>
        </form>

        {SHOW_DEMO_ACCOUNTS && (
        <div className="card" style={{ padding: 24, background: "var(--bg)" }}>
          <div className="t-md t-semibold" style={{ marginBottom: 4 }}>Demo accounts</div>
          <div className="t-xs t-muted" style={{ marginBottom: 14 }}>
            Click to autofill. These are bootstrapped on first run.
          </div>
          {DEMO_ACCOUNTS.map(([em, pw, desc]) => (
            <button
              type="button"
              key={em}
              onClick={() => { setEmail(em); setPassword(pw); }}
              className="card"
              style={{
                padding: "10px 12px", marginBottom: 8, width: "100%", textAlign: "left",
                cursor: "pointer", background: "var(--bg-subtle)",
              }}
            >
              <div className="t-sm t-mono t-medium">{em}</div>
              <div className="t-xs t-subtle" style={{ marginTop: 2 }}>{desc}</div>
              <div className="t-xs t-mono t-subtle" style={{ marginTop: 4 }}>password: {pw}</div>
            </button>
          ))}
        </div>
        )}
      </div>
    </div>
  );
}
