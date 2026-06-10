"use client";

import * as React from "react";
import { createPortal } from "react-dom";

import { Ico } from "@/components/icons";
import { Av } from "@/components/primitives";
import { useAuth, type Me } from "@/lib/auth-context";

/// Sidebar footer — shows the signed-in user or a "Sign in" CTA.  Drops into
/// the existing sidebar layout in place of the old hardcoded "Anong K." block.
export function UserMenu() {
  const { user, loading, logout } = useAuth();
  const [open, setOpen] = React.useState(false);
  const [pos, setPos] = React.useState<{ top: number; left: number; width: number } | null>(null);
  const btnRef = React.useRef<HTMLButtonElement>(null);
  const menuRef = React.useRef<HTMLDivElement>(null);

  // Position the menu with fixed coords anchored to the button, opening UPWARD
  // (above the trigger) and left-aligned to it. Rendered in a portal on
  // document.body so it can NEVER be clipped by the sidebar's overflow.
  React.useEffect(() => {
    if (!open) { setPos(null); return; }
    const place = () => {
      const b = btnRef.current?.getBoundingClientRect();
      if (!b) return;
      // First-frame estimate; corrected by the layout effect below once the
      // menu is measurable.
      const menuH = menuRef.current?.getBoundingClientRect().height ?? 140;
      setPos({
        top: Math.max(8, b.top - menuH - 4),
        left: b.left,
        width: b.width,
      });
    };
    place();
    const close = () => setOpen(false);
    const onDocDown = (e: MouseEvent) => {
      if (btnRef.current?.contains(e.target as Node) || menuRef.current?.contains(e.target as Node)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", onDocDown);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      document.removeEventListener("mousedown", onDocDown);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [open]);

  React.useLayoutEffect(() => {
    if (!open || !pos) return;
    const m = menuRef.current?.getBoundingClientRect();
    const b = btnRef.current?.getBoundingClientRect();
    if (!m || !b) return;
    const top = Math.max(8, b.top - m.height - 4);
    if (Math.abs(top - pos.top) > 1) setPos({ ...pos, top });
  }, [open, pos]);

  if (loading) {
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 10px 4px", opacity: 0.5 }}>
        <span className="av sm" style={{ background: "var(--bg-strong)" }}>·</span>
        <span className="t-sm t-subtle">Loading…</span>
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
    <div style={{ position: "relative" }}>
      <button
        ref={btnRef}
        aria-haspopup="menu"
        aria-expanded={open}
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
      {open && pos && typeof document !== "undefined" && createPortal(
        <div ref={menuRef} className="card" role="menu" style={{
          position: "fixed", top: pos.top, left: pos.left, width: pos.width,
          padding: 6, boxShadow: "var(--sh-popover)", zIndex: 200,
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
        </div>,
        document.body,
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
