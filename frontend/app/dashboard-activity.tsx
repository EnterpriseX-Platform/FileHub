"use client";

import * as React from "react";

import { Av, Ft } from "@/components/primitives";
import { fmtAgo } from "@/lib/format";
import type { Activity } from "@/lib/api";

// Tabs filter the recent-activity feed by the action verb. Heuristic matching
// on `action` text (uploads vs review/approve/reject) — good enough for the
// dashboard's small recent list.
const TABS: Array<{ key: string; label: string; match: (a: Activity) => boolean }> = [
  { key: "all",     label: "All",     match: () => true },
  { key: "uploads", label: "Uploads", match: (a) => /upload|add|version|restore/i.test(a.action) },
  { key: "reviews", label: "Reviews", match: (a) => /review|approv|reject|comment/i.test(a.action) },
];

export function DashboardActivity({ activity }: { activity: Activity[] }) {
  const [tab, setTab] = React.useState("all");
  const items = activity.filter(TABS.find((t) => t.key === tab)!.match);

  return (
    <>
      <div style={{ padding: "14px 16px 8px", display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <h2 className="t-lg t-semibold">Recent activity</h2>
        <div style={{ display: "flex", gap: 6 }}>
          {TABS.map((t) => (
            <button
              key={t.key}
              className={"btn xs" + (tab === t.key ? "" : " ghost")}
              onClick={() => setTab(t.key)}
            >
              {t.label}
            </button>
          ))}
        </div>
      </div>
      <div>
        {items.length === 0 ? (
          <div className="t-sm t-subtle" style={{ padding: "16px 24px" }}>
            No activity yet — uploads, reviews, and shares appear here as your team works.
          </div>
        ) : items.map((a, i) => (
          <React.Fragment key={a.id ?? i}>
            {(i === 0 || dayLabel(a.created_at) !== dayLabel(items[i - 1].created_at)) && (
              <div className="t-xs t-subtle t-medium" style={{
                padding: "8px 16px 2px", letterSpacing: "0.05em", textTransform: "uppercase",
                borderTop: i ? "1px solid var(--border)" : "none",
              }}>
                {dayLabel(a.created_at)}
              </div>
            )}
          <div
            style={{
              display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap",
              padding: "10px 16px",
              borderTop: "1px solid var(--border-subtle)",
            }}
          >
            <Av name={a.actor} tone={a.actor_tone} />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="t-base t-trunc">
                <span className="t-semibold">{a.actor}</span>{" "}
                <span className="t-muted">{a.action}</span>{" "}
                <span className="t-medium">{a.target}</span>
              </div>
              {a.system_id && <div className="t-xs t-subtle" style={{ marginTop: 1 }}>{a.system_id}</div>}
            </div>
            {a.target_type && <Ft type={a.target_type} />}
            <div className="t-sm t-subtle t-tabular" style={{ width: 80, textAlign: "right" }}>{fmtAgo(a.created_at)}</div>
          </div>
          </React.Fragment>
        ))}
      </div>
    </>
  );
}

/// "Today" / "Yesterday" / a short date — feed group headers.
function dayLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((startOf(today) - startOf(d)) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
