"use client";

import { useRouter, useSearchParams } from "next/navigation";
import * as React from "react";

import { useAuth } from "@/lib/auth-context";

export const dynamic = "force-dynamic";

const DEMO_ACCOUNTS = [
  ["admin@acme.go.th",  "Admin · hard-delete + full RBAC"],
  ["anong@acme.go.th",  "Editor · upload, edit, share"],
  ["viewer@acme.go.th", "Viewer · read-only"],
];

// The test-account panel prefills the email so you can pick a role quickly; the
// password is never shown or autofilled (that plaintext-credential display is
// what made the login read like a mockup). It's a DEV convenience, shown only
// outside production builds. Override with NEXT_PUBLIC_SHOW_DEMO_ACCOUNTS=1/0.
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
        gridTemplateColumns: SHOW_DEMO_ACCOUNTS ? "repeat(auto-fit, minmax(340px, 1fr))" : "1fr",
        gap: 24,
        maxWidth: SHOW_DEMO_ACCOUNTS ? 880 : 400,
        width: "100%",
      }}>
        <form onSubmit={submit} className="card" style={{ padding: 36, display: "flex", flexDirection: "column", gap: 18, boxShadow: "var(--sh-3)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 4 }}>
            <span style={{ width: 44, height: 44, borderRadius: "var(--r-5)", background: "var(--accent)", color: "var(--on-accent)", display: "flex", alignItems: "center", justifyContent: "center", fontWeight: 700, fontSize: 20 }}>F</span>
            <div>
              <div className="t-2xl t-semibold">File Hub</div>
              <div className="t-sm t-muted">acme.go.th · Digital Content Platform</div>
            </div>
          </div>

          <div>
            <label htmlFor="email" className="t-sm t-muted t-medium" style={{ display: "block", marginBottom: 6 }}>Email</label>
            <div className="field" style={{ width: "100%", height: 42, fontSize: "var(--t-md)" }}>
              <input
                id="email"
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
            <label htmlFor="password" className="t-sm t-muted t-medium" style={{ display: "block", marginBottom: 6 }}>Password</label>
            <div className="field" style={{ width: "100%", height: 42, fontSize: "var(--t-md)" }}>
              <input
                id="password"
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
            <div className="t-sm" style={{ color: "var(--danger)", marginTop: -6 }}>{error}</div>
          )}

          <button type="submit" className="btn primary" disabled={busy} style={{ justifyContent: "center", height: 42, fontSize: "var(--t-md)" }}>
            {busy ? "Signing in…" : "Sign in"}
          </button>

          <div className="t-sm t-subtle">
            Trouble signing in? Contact your workspace admin.
          </div>
        </form>

        {SHOW_DEMO_ACCOUNTS && (
        <div className="card" style={{ padding: 24, background: "var(--bg)" }}>
          <div className="t-md t-semibold" style={{ marginBottom: 4 }}>Test accounts</div>
          <div className="t-xs t-muted" style={{ marginBottom: 14 }}>
            Click an account to fill its email, then enter the password.
          </div>
          {DEMO_ACCOUNTS.map(([em, desc]) => (
            <button
              type="button"
              key={em}
              onClick={() => setEmail(em)}
              className="card"
              style={{
                padding: "10px 12px", marginBottom: 8, width: "100%", textAlign: "left",
                cursor: "pointer", background: "var(--bg-subtle)",
              }}
            >
              <div className="t-sm t-mono t-medium">{em}</div>
              <div className="t-xs t-subtle" style={{ marginTop: 2 }}>{desc}</div>
            </button>
          ))}
        </div>
        )}
      </div>
    </div>
  );
}
