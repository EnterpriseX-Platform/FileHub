"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Av } from "@/components/primitives";
import { useAuth, type Me } from "@/lib/auth-context";

/// Sidebar footer — shows the signed-in user or a "Sign in" CTA.  Drops into
/// the existing sidebar layout in place of the old hardcoded "Anong K." block.
export function UserMenu() {
  const { user, loading, logout } = useAuth();
  const [open, setOpen] = React.useState(false);
  const ref = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);

  if (loading) {
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 10px 4px", opacity: 0.5 }}>
        <span className="av sm" style={{ background: "var(--bg-strong)" }}>·</span>
        <span className="t-sm t-subtle">loading…</span>
      </div>
    );
  }

  if (!user) {
    return (
      <a href="/login" className="btn primary" style={{ margin: 8, justifyContent: "center" }}>
        Sign in
      </a>
    );
  }

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button
        onClick={() => setOpen((v) => !v)}
        style={{
          display: "flex", alignItems: "center", gap: 8, padding: "8px 10px 4px",
          background: "transparent", border: 0, width: "100%", cursor: "pointer",
        }}
      >
        <Av name={user.display_name} tone={tone(user.avatar_tone)} />
        <div style={{ flex: 1, minWidth: 0, textAlign: "left" }}>
          <div className="t-sm t-semibold t-trunc">{user.display_name}</div>
          <div className="t-xs t-subtle t-trunc">{user.role}</div>
        </div>
        <Ico.moreV className="icon sm" />
      </button>
      {open && (
        <div className="card" style={{
          position: "absolute", bottom: "calc(100% + 4px)", left: 8, right: 8,
          padding: 6, boxShadow: "var(--sh-popover)", zIndex: 100,
        }}>
          <UserItem user={user} />
          <div className="divider" style={{ margin: "4px 0" }} />
          <button
            onClick={() => { setOpen(false); logout(); }}
            className="btn xs ghost"
            style={{ width: "100%", justifyContent: "flex-start", padding: "6px 8px" }}
          >
            <Ico.x className="icon sm" /> Sign out
          </button>
        </div>
      )}
    </div>
  );
}

function UserItem({ user }: { user: Me }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 8px" }}>
      <Av name={user.display_name} tone={tone(user.avatar_tone)} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="t-sm t-semibold t-trunc">{user.display_name}</div>
        <div className="t-xs t-subtle t-trunc">{user.email}</div>
      </div>
    </div>
  );
}

function tone(t: string): "indigo" | "emerald" | "amber" | "rose" | "violet" | "cyan" | "fuchsia" | "slate" {
  const allowed = ["indigo", "emerald", "amber", "rose", "violet", "cyan", "fuchsia", "slate"] as const;
  return (allowed as readonly string[]).includes(t) ? (t as typeof allowed[number]) : "slate";
}
