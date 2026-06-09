import { Av, Ft, Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeActivity, safeSystems } from "@/lib/api";
import { fmtAgo } from "@/lib/format";

import { SettingsNav } from "../nav";
import { loadSettingsCtx } from "../_shared";

/// Audit log tab — the workspace activity feed, surfaced as an admin-facing
/// audit trail.  We reuse /api/activity (which is already DB-backed and
/// records every upload, delete, restore, share, workflow decision, system
/// CRUD, etc.) instead of building a parallel "audit_log" table.
export default async function SettingsAuditPage() {
  // /api/activity + /api/systems are private routes — server-side fetches
  // need the inbound session cookie forwarded or the backend 401s and the
  // page renders "Audit log · 0" with an empty table.  Mirrors the pattern
  // every other settings/* page uses (see settings/members/page.tsx).
  const { cookieHeader } = await loadSettingsCtx();
  const [events, systems] = await Promise.all([
    safeActivity(200, cookieHeader),
    safeSystems(cookieHeader),
  ]);
  return (
    <div className="scr">
      <Sidebar nav="settings" systems={systems} />
      <TopBar crumbs={["Settings", "Audit log"]} title={`Audit log · ${events.length}`} />
      <div className="main" style={{ display: "grid", gridTemplateColumns: "220px 1fr", overflow: "hidden" }}>
        <SettingsNav active="audit" />
        <div style={{ overflow: "auto", padding: "24px 32px" }}>
          <div style={{ maxWidth: 1100 }}>
            <div className="t-3xl t-semibold">Audit log</div>
            <div className="t-sm t-muted" style={{ marginTop: 4, marginBottom: 24 }}>
              Every workspace event from the <span className="t-mono">activity</span> table — uploads, deletions, workflow decisions, bucket CRUD.
            </div>

            <div className="card" style={{ padding: 0 }}>
              <table className="tbl">
                <thead>
                  <tr>
                    <th style={{ width: 220 }}>Actor</th>
                    <th>Action</th>
                    <th>Target</th>
                    <th style={{ width: 130 }}>System</th>
                    <th style={{ width: 120 }}>When</th>
                  </tr>
                </thead>
                <tbody>
                  {events.length === 0 ? (
                    <tr><td colSpan={5} style={{ padding: 32, textAlign: "center", color: "var(--text-subtle)" }}>No activity recorded yet — uploads, deletions, shares, and workflow decisions appear here automatically.</td></tr>
                  ) : events.map((a) => (
                    <tr key={a.id}>
                      <td>
                        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                          <Av name={a.actor} tone={a.actor_tone} />
                          <span className="t-sm t-medium">{a.actor}</span>
                        </div>
                      </td>
                      <td className="t-sm">{a.action}</td>
                      <td>
                        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                          {a.target_type && <Ft type={a.target_type} />}
                          <span className="t-sm t-trunc">{a.target ?? "—"}</span>
                        </div>
                      </td>
                      <td>{a.system_id ? <Pill tone={a.actor_tone} sm><span className="dot" />{a.system_id}</Pill> : <span className="t-subtle">—</span>}</td>
                      <td className="t-xs t-muted">{fmtAgo(a.created_at)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
