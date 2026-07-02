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
    <div className="auth-split">
      {/* Left — AI-first product hero. The chat mock mirrors real /ask output
          (grounded RAG with citations), so it advertises shipping features. */}
      <div className="auth-hero">
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span className="auth-logo">F</span>
          <span className="t-lg t-semibold">File Hub</span>
          <span className="auth-feat" style={{ marginLeft: 4 }}>✦ AI-native</span>
        </div>
        <h1 className="auth-hero-h1">
          Every document,<br /><span className="grad-text-hero">one question away.</span>
        </h1>
        <p className="auth-hero-sub">
          Semantic search, grounded answers with citations, Thai + English OCR,
          e-signatures and approval workflows — in one workspace.
        </p>

        <div className="auth-chat">
          <div className="auth-bubble q">งบประมาณโครงการ DMS เท่าไหร่ และเบิกจ่ายไปแล้วเท่าไร?</div>
          <div className="auth-bubble a">
            งบประมาณรวมทั้งสิ้น <b>4,200,000 บาท</b><span className="auth-cite">1</span> เบิกจ่ายแล้ว{" "}
            <b>1,260,000 บาท</b> (ร้อยละ 30 งวดที่ 1)<span className="auth-cite">2</span>
            <div className="auth-chat-meta" style={{ marginTop: 8 }}>
              <span>✦ grounded</span>·<span>TOR-summary-DMS-2026</span>·<span>meeting-minutes 4/2569</span>
            </div>
          </div>
        </div>

        <div className="auth-feats">
          <span className="auth-feat">⌕ Search by meaning</span>
          <span className="auth-feat">⌘K everywhere</span>
          <span className="auth-feat">OCR ไทย · EN</span>
          <span className="auth-feat">✍ E-sign & workflows</span>
        </div>
      </div>

      {/* Right — sign-in pane. */}
      <div className="auth-pane">
        <form onSubmit={submit} className="auth-card" style={{ padding: 36, display: "flex", flexDirection: "column", gap: 18, width: "100%", maxWidth: 400 }}>
          <div style={{ marginBottom: 4 }}>
            <div className="t-2xl t-semibold" style={{ lineHeight: 1.2 }}>
              Welcome back
            </div>
            <div className="t-sm t-muted">acme.go.th · Digital Content Platform</div>
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
        <div className="auth-card" style={{ padding: 20, width: "100%", maxWidth: 400 }}>
          <div className="t-sm t-semibold" style={{ marginBottom: 2 }}>Test accounts</div>
          <div className="t-xs t-muted" style={{ marginBottom: 10 }}>
            Click an account to fill its email, then enter the password.
          </div>
          {DEMO_ACCOUNTS.map(([em, desc]) => (
            <button
              type="button"
              key={em}
              onClick={() => setEmail(em)}
              className="card"
              style={{
                padding: "8px 12px", marginBottom: 6, width: "100%", textAlign: "left",
                cursor: "pointer", background: "var(--bg-subtle)",
              }}
            >
              <div className="t-sm t-mono t-medium">{em}</div>
              <div className="t-xs t-subtle" style={{ marginTop: 1 }}>{desc}</div>
            </button>
          ))}
        </div>
        )}
      </div>
    </div>
  );
}
