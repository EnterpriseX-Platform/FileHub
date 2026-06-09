"use client";

import * as React from "react";

import { Av, Ft, Pill } from "@/components/primitives";
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
        <div className="t-lg t-semibold">Recent activity</div>
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
          <div
            key={a.id ?? i}
            style={{
              display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap",
              padding: "10px 16px",
              borderTop: "1px solid var(--border)",
              background: i === 0 ? "var(--bg-subtle)" : "transparent",
            }}
          >
            <Av name={a.actor} tone={a.actor_tone} />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="t-base">
                <span className="t-semibold">{a.actor}</span>{" "}
                <span className="t-muted">{a.action}</span>{" "}
                <span className="t-medium">{a.target}</span>
              </div>
              <div className="t-sm t-muted" style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 2 }}>
                <Pill tone={a.actor_tone}><span className="dot" />{a.system_id ?? "—"}</Pill>
              </div>
            </div>
            {a.target_type && <Ft type={a.target_type} />}
            <div className="t-sm t-subtle t-tabular" style={{ width: 80, textAlign: "right" }}>{fmtAgo(a.created_at)}</div>
          </div>
        ))}
      </div>
    </>
  );
}
