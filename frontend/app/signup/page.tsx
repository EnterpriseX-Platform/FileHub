"use client";

import { ThemeToggle } from "@/lib/theme-context";

import * as React from "react";
import Link from "next/link";

import { useBranding } from "@/lib/branding";
import { Mark, Note, TextField } from "@/components/cx/ui";
import "../carbon.css";

/// Ask for an account. It starts switched off; an admin turns it on in
/// Admin → People. Same card as the sign-in page.
export default function SignUpPage() {
  const brand = useBranding();
  const [name, setName] = React.useState("");
  const [email, setEmail] = React.useState("");
  const [password, setPassword] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [done, setDone] = React.useState(false);
  const tagline = brand.workspace_display && brand.workspace_display.replace(/\s/g, "").toLowerCase() !== "filehub" ? brand.workspace_display : "Central document platform";

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true); setError(null);
    try {
      const r = await fetch("/filehub/api/auth/signup", { method: "POST", headers: { "content-type": "application/json" },
        body: JSON.stringify({ display_name: name, email, password }) });
      if (!r.ok) {
        const j = await r.json().catch(() => null);
        throw new Error(r.status === 404 ? "Sign-up is turned off." : j?.error ?? `Couldn't create the account (${r.status}).`);
      }
      setDone(true);
    } catch (err) { setError((err as Error).message); }
    setBusy(false);
  };

  return (
    <main className="cx-app cx-login">
      {brand.env_label && <div className="cx-login-env">{brand.env_label}</div>}
      <div className="cx-login-corner"><ThemeToggle /></div>
      <div className="cx-login-stack">
        <form onSubmit={submit} className="cx-login-card cx-on-layer">
          <div className="cx-login-brand">
            <Mark size={32} />
            <span>{brand.brand_name}</span>
          </div>
          <h1 className="cx-login-title">Create account</h1>
          <p className="cx-login-tagline">{tagline}</p>
          {brand.self_signup !== "on" ? (
            <Note kind="info">Sign-up is off. Ask an administrator for an account.</Note>
          ) : done ? (
            <Note kind="success">Account requested for {email}. You can sign in once an administrator turns it on.</Note>
          ) : (<>
            <TextField id="name" label="Your name" value={name} onChange={(e) => setName(e.target.value)} required autoFocus autoComplete="name" />
            <TextField id="email" label="Email" type="email" value={email} onChange={(e) => setEmail(e.target.value)}
              placeholder={brand.login_email_placeholder} required autoComplete="email" />
            <TextField id="password" label="Password" type="password" value={password} onChange={(e) => setPassword(e.target.value)}
              required minLength={8} autoComplete="new-password" helper="At least 8 characters." />
            {error && <Note kind="error">{error}</Note>}
            <button type="submit" className="cx-btn cx-btn-primary cx-login-submit" disabled={busy}>{busy ? "Creating account…" : "Create account"}</button>
          </>)}
          <p className="cx-login-foot">Already have an account? <Link href="/login">Sign in</Link></p>
        </form>
      </div>
    </main>
  );
}
