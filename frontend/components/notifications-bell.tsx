"use client";

import Link from "next/link";
import * as React from "react";

import { Ico } from "@/components/icons";
import type { Notification } from "@/lib/api";
import { fmtAgo } from "@/lib/format";

/// Topbar bell with unread-count badge + dropdown.  Replaces the previous
/// decorative bell button on every page.  Polls /api/notifications/unread-count
/// every 30s when the popover is closed; opens to a live /api/notifications
/// list with a one-click "mark read" per row.
export function NotificationsBell({ tone = "ghost" }: { tone?: "ghost" | "icon" }) {
  const [open,    setOpen]    = React.useState(false);
  const [unread,  setUnread]  = React.useState(0);
  const [items,   setItems]   = React.useState<Notification[] | null>(null);
  const ref = React.useRef<HTMLDivElement>(null);

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

  // Close on outside click.
  React.useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);

  const loadList = async () => {
    try {
      const r = await fetch("/filehub/api/notifications?limit=20", { credentials: "include", cache: "no-store" });
      if (r.ok) setItems(await r.json());
    } catch { /* keep last */ }
  };

  const markRead = async (id: string) => {
    await fetch(`/filehub/api/notifications/${encodeURIComponent(id)}/read`, { method: "POST", credentials: "include" });
    setItems((cur) => cur?.map((n) => n.id === id ? { ...n, read_at: new Date().toISOString() } : n) ?? cur);
    setUnread((u) => Math.max(0, u - 1));
  };

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button
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
            color: "#fff", fontSize: 9, fontWeight: 700, lineHeight: "14px",
            textAlign: "center",
          }}>
            {unread > 99 ? "99+" : unread}
          </span>
        )}
      </button>

      {open && (
        <div className="card" style={{
          position: "absolute", top: "calc(100% + 6px)", right: 0,
          width: 360, maxHeight: 480, overflow: "auto", padding: 0,
          boxShadow: "var(--sh-popover)", zIndex: 100,
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
                  <Link href={n.link} className="btn xs ghost" onClick={() => setOpen(false)}>View</Link>
                )}
                {!n.read_at && (
                  <button className="btn xs ghost" onClick={() => markRead(n.id)}>Mark read</button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
