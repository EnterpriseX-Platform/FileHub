"use client";

import { ThemeToggle } from "@/lib/theme-context";

import { useRouter, useSearchParams } from "next/navigation";

/// Only paths inside FileHub — never another site (`//x`, `/\\x`, `https://…`).
const safeNext = (n: string | null) => (n && n.startsWith("/") && !n.startsWith("//") && !n.startsWith("/\\") ? n : "/");
import * as React from "react";

import { useAuth } from "@/lib/auth-context";
import { useBranding } from "@/lib/branding";
import Link from "next/link";
import { Note, TextField } from "@/components/cx/ui";
import "../carbon.css";

export const dynamic = "force-dynamic";

// No demo accounts on the login page: their passwords are in the public README,
// so listing them on a real deployment points attackers at them.
const DEMO_ACCOUNTS: Array<[string, string]> = [];

// The test-account panel prefills the email so you can pick a role quickly; the
// password is never shown or autofilled (that plaintext-credential display is
// what made the login read like a mockup). It's a DEV convenience, shown only
// outside production builds. Override with NEXT_PUBLIC_SHOW_DEMO_ACCOUNTS=1/0.
const SHOW_DEMO_ACCOUNTS = DEMO_ACCOUNTS.length > 0;

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
  const brand = useBranding();
  const [email, setEmail]       = React.useState("");
  const [password, setPassword] = React.useState("");
  const [busy, setBusy]         = React.useState(false);
  const [error, setError]       = React.useState<string>("");

  // Already logged in?  Bounce straight to the redirect target.
  React.useEffect(() => {
    if (!loading && user) {
      router.replace(safeNext(params.get("next")));
    }
  }, [loading, user, router, params]);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    // Required fields are checked here, before anything is sent.
    if (!email.trim() && !password) { setError("กรุณากรอกอีเมลและรหัสผ่าน"); return; }
    if (!email.trim()) { setError("กรุณากรอกอีเมล"); return; }
    if (!/^[^\s@]+@[^\s@]+$/.test(email.trim())) { setError("รูปแบบอีเมลไม่ถูกต้อง"); return; }
    if (!password) { setError("กรุณากรอกรหัสผ่าน"); return; }
    setBusy(true);
    try {
      const r = await fetch("/filehub/api/auth/login", {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email, password }),
      });
      if (r.status === 401) {
        setError("อีเมลหรือรหัสผ่านไม่ถูกต้อง");
        return;
      }
      if (r.status === 429) { setError("ลองเข้าสู่ระบบผิดหลายครั้งเกินไป กรุณารอ 1 นาทีแล้วลองใหม่"); return; }
      if (!r.ok) throw new Error(`เข้าสู่ระบบไม่สำเร็จ (${r.status})`);
      reload();
      router.replace(safeNext(params.get("next")));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  // Product sign-in, one
  // card, product mark and name, the workspace as the line under the heading.
  const tagline = brand.workspace_display && brand.workspace_display.replace(/\s/g, "").toLowerCase() !== "filehub" ? brand.workspace_display : "Central document platform";
  return (
    <main className="cx-app cx-login">
      {brand.env_label && <div className="cx-login-env">{brand.env_label}</div>}
      <div className="cx-login-corner"><ThemeToggle /></div>
      <div className="cx-login-stack">
        <form onSubmit={submit} noValidate className="cx-login-card cx-on-layer">
          <div className="cx-login-brand">
            <svg viewBox="0 0 32 32" width="32" height="32" aria-hidden="true">
              <rect x="2" y="2" width="28" height="28" rx="7" fill="#0f62fe" />
              <path d="M9 11.5h5l1.5 2H23v8.5H9z" fill="none" stroke="#fff" strokeWidth="2" strokeLinejoin="round" />
            </svg>
            <span>FileHub</span>
          </div>
          <h1 className="cx-login-title">เข้าสู่ระบบ</h1>
          <p className="cx-login-tagline">{tagline}</p>
          <TextField id="email" label="อีเมล" type="email" value={email} onChange={(e) => setEmail(e.target.value)}
            placeholder={brand.login_email_placeholder} autoFocus required autoComplete="username" />
          <TextField id="password" label="รหัสผ่าน" type="password" value={password} onChange={(e) => setPassword(e.target.value)}
            required autoComplete="current-password" />
          {error && <Note kind="error">{error}</Note>}
          <button type="submit" className="cx-btn cx-btn-primary cx-login-submit" disabled={busy}>{busy ? "กำลังเข้าสู่ระบบ" : "เข้าสู่ระบบ"}</button>
          {brand.self_signup === "on"
            ? <p className="cx-login-foot">No account? <Link href="/signup">Sign up</Link></p>
            : null}
        </form>
        {SHOW_DEMO_ACCOUNTS && (
          <div className="cx-login-card cx-on-layer">
            <div style={{ fontWeight: 600 }}>Test accounts</div>
            {DEMO_ACCOUNTS.map(([em, desc]) => (
              <button type="button" key={em} className="cx-login-demo" onClick={() => setEmail(em)}>
                <span style={{ fontFamily: "var(--cx-mono)" }}>{em}</span><small>{desc}</small>
              </button>
            ))}
          </div>
        )}
      </div>
    </main>
  );
}
