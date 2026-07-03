"use client";

import Link from "next/link";
import * as React from "react";
import { createPortal } from "react-dom";

import { Ico } from "@/components/icons";
import type { Notification } from "@/lib/api";
import { fmtAgo } from "@/lib/format";
import { useViewMode } from "@/lib/view-mode";

/// Notification links are stored as workspace paths (/files/:id) or everyday
/// paths (/f/:id) depending on which handler wrote them — remap to the shell
/// the user is actually in so a "View" from the everyday bell never lands in
/// the Admin console (and vice versa).
function linkForMode(link: string, everyday: boolean): string {
  const fileMatch = link.match(/^\/files\/([^/?#]+)$/);
  if (everyday && fileMatch) return `/f/${fileMatch[1]}`;
  const edayMatch = link.match(/^\/f\/([^/?#]+)$/);
  if (!everyday && edayMatch) return `/files/${edayMatch[1]}`;
  return link;
}

/// Topbar bell with unread-count badge + dropdown.  Replaces the previous
/// decorative bell button on every page.  Polls /api/notifications/unread-count
/// every 30s when the popover is closed; opens to a live /api/notifications
/// list with a one-click "mark read" per row.
export function NotificationsBell({ tone = "ghost" }: { tone?: "ghost" | "icon" }) {
  const { mode: viewMode } = useViewMode();
  const [open,    setOpen]    = React.useState(false);
  const [unread,  setUnread]  = React.useState(0);
  const [items,   setItems]   = React.useState<Notification[] | null>(null);
  const [pos,     setPos]     = React.useState<{ top: number; right: number } | null>(null);
  const btnRef  = React.useRef<HTMLButtonElement>(null);
  const menuRef = React.useRef<HTMLDivElement>(null);

  // Poll unread count so the badge stays fresh even before the user opens.
  React.useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const r = await fetch("/filehub/api/notifications/unread-count", { credentials: "include", cache: "no-store" });
        if (!cancelled && r.ok) {
          const { unread } = await r.json();
          setUnread(Number(unread) || 0);
        }
      } catch { /* badge stays where it was */ }
    };
    tick();
    const h = setInterval(tick, 30_000);
    return () => { cancelled = true; clearInterval(h); };
  }, []);

  // Anchor the popover to the bell with fixed coords and render it in a portal on
  // document.body so it can NEVER be clipped by the topbar's overflow. Preserves
  // this menu's downward, right-aligned direction (top from the button's bottom,
  // right edge aligned to the bell). Closes on outside-mousedown / Escape /
  // scroll(capture) / resize, mirroring the RowMenu pattern in files-table.tsx.
  React.useEffect(() => {
    if (!open) { setPos(null); return; }
    const place = () => {
      const b = btnRef.current?.getBoundingClientRect();
      if (!b) return;
      setPos({
        top: b.bottom + 6,
        right: Math.max(8, window.innerWidth - b.right),
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

  const loadList = async () => {
    try {
      const r = await fetch("/filehub/api/notifications?limit=20", { credentials: "include", cache: "no-store" });
      if (r.ok) setItems(await r.json());
    } catch { /* keep last */ }
  };

  const markRead = async (id: string) => {
    try {
      const r = await fetch(`/filehub/api/notifications/${encodeURIComponent(id)}/read`, { method: "POST", credentials: "include" });
      if (!r.ok) return; // leave the row unread on failure
      setItems((cur) => cur?.map((n) => n.id === id ? { ...n, read_at: new Date().toISOString() } : n) ?? cur);
      setUnread((u) => Math.max(0, u - 1));
    } catch { /* network error — keep the row unread, UI unaffected */ }
  };

  return (
    <div style={{ position: "relative" }}>
      <button
        ref={btnRef}
        className={`btn ${tone} icon`}
        onClick={() => { setOpen((v) => !v); if (!open) void loadList(); }}
        aria-label={unread > 0 ? `${unread} unread notifications` : "notifications"}
        style={{ position: "relative" }}
      >
        <Ico.bell />
        {unread > 0 && (
          <span style={{
            position: "absolute", top: 2, right: 2, minWidth: 14, height: 14,
            padding: "0 4px", borderRadius: 7, background: "var(--c-rose)",
            color: "var(--on-accent)", fontSize: 9, fontWeight: 700, lineHeight: "14px",
            textAlign: "center",
          }}>
            {unread > 99 ? "99+" : unread}
          </span>
        )}
      </button>

      {open && pos && typeof document !== "undefined" && createPortal(
        <div ref={menuRef} className="card" style={{
          position: "fixed", top: pos.top, right: pos.right,
          width: 360, maxHeight: 480, overflow: "auto", padding: 0,
          boxShadow: "var(--sh-popover)", zIndex: 200,
          borderRadius: "var(--r-5)",
          background: "color-mix(in srgb, var(--bg) 90%, transparent)",
          backdropFilter: "blur(16px) saturate(1.5)",
          WebkitBackdropFilter: "blur(16px) saturate(1.5)",
          animation: "cmdIn .16s var(--ease)",
        }}>
          <div style={{ padding: "10px 14px", borderBottom: "1px solid var(--border)", display: "flex", alignItems: "center", justifyContent: "space-between" }}>
            <div className="t-sm t-semibold">Notifications</div>
            <div className="t-xs t-subtle">{unread} unread</div>
          </div>

          {items === null ? (
            <div className="t-sm t-subtle" style={{ padding: 16 }}>Loading…</div>
          ) : items.length === 0 ? (
            <div className="t-sm t-subtle" style={{ padding: 16, textAlign: "center" }}>All caught up — review requests, approvals, and mentions show up here.</div>
          ) : items.map((n) => (
            <div
              key={n.id}
              style={{
                padding: "10px 14px", borderTop: "1px solid var(--border-subtle)",
                background: n.read_at ? "transparent" : "var(--bg-subtle)",
                display: "flex", gap: 8,
              }}
            >
              <span style={{
                width: 8, height: 8, borderRadius: 4, flexShrink: 0, marginTop: 6,
                background: n.read_at ? "var(--text-subtle)" : "var(--c-rose)",
              }} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="t-sm t-semibold t-trunc">{n.title}</div>
                {n.body && <div className="t-xs t-muted">{n.body}</div>}
                <div className="t-xs t-subtle" style={{ marginTop: 2 }}>{fmtAgo(n.created_at)}</div>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 2, alignItems: "flex-end" }}>
                {n.link && (
                  <Link href={linkForMode(n.link, viewMode === "everyday")} className="btn xs ghost" onClick={() => setOpen(false)}>View</Link>
                )}
                {!n.read_at && (
                  <button className="btn xs ghost" onClick={() => markRead(n.id)}>Mark read</button>
                )}
              </div>
            </div>
          ))}
        </div>,
        document.body,
      )}
    </div>
  );
}
