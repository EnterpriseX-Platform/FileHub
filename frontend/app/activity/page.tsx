import { Pager } from "@/components/pager";
import { Av, Ft, Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeActivity, safeOrgs, safeStats, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtAgo } from "@/lib/format";

import { SyncButton } from "../sync-button";

export default async function ActivityPage({ searchParams }: { searchParams?: Promise<Record<string, string | string[] | undefined>> }) {
  const sp = (await searchParams) ?? {};
  const { cookieHeader } = await loadServerCtx();
  const [activity, systems, stats] = await Promise.all([
    safeActivity(200, cookieHeader),
    safeSystems(cookieHeader),
    safeStats(cookieHeader),
  ]);
  const orgs = systems[0] ? await safeOrgs(systems[0].id, cookieHeader) : [];

  const PAGE_SIZE = 50;
  const total = activity.length;
  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const page = Math.min(Math.max(0, parseInt(typeof sp.page === "string" ? sp.page : "0", 10) || 0), pageCount - 1);
  const pageRows = activity.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  return (
    <div className="scr">
      <Sidebar nav="activity" systems={systems} stats={stats} orgs={orgs} />
      <TopBar
        crumbs={["Workspace", "Activity"]}
        actions={<SyncButton />}
      />
      <div className="main main-pad" style={{ overflow: "auto" }}>
        <div className="page">
          <div className="t-3xl t-semibold" style={{ marginBottom: "var(--sp-1)" }}>Activity</div>
          <div className="t-sm t-muted" style={{ marginBottom: "var(--sp-4)" }}>{activity.length} events across all systems</div>

          <div className="card" style={{ padding: 0 }} role="list">
            {activity.length === 0 && (
              <div style={{ padding: "var(--sp-8)", textAlign: "center", color: "var(--text-subtle)" }}>
                No events yet — uploads, comments, approvals, and shares stream in here as your team works. <a href="/upload" style={{ color: "var(--accent)" }}>Upload a file</a> to generate events.
              </div>
            )}
            {pageRows.map((a, i) => (
              <div
                key={a.id}
                role="listitem"
                style={{
                  display: "flex", alignItems: "center", gap: "var(--sp-3)",
                  padding: "var(--sp-3) var(--sp-4)", borderTop: i ? "1px solid var(--border)" : "none",
                }}
              >
                <Av name={a.actor} tone={a.actor_tone} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="t-base">
                    <span className="t-semibold">{a.actor}</span>{" "}
                    <span className="t-muted">{a.action}</span>{" "}
                    <span className="t-medium">{a.target}</span>
                  </div>
                  <div className="t-sm t-muted" style={{ marginTop: "var(--sp-1)" }}>
                    <Pill tone={a.actor_tone}><span className="dot" />{a.system_id ?? "—"}</Pill>
                  </div>
                </div>
                {a.target_type && <Ft type={a.target_type} />}
                <div className="t-sm t-subtle t-tabular" style={{ minWidth: 90, flexShrink: 0, whiteSpace: "nowrap", textAlign: "right" }}>{fmtAgo(a.created_at)}</div>
              </div>
            ))}
          </div>
          <Pager page={page} pageSize={PAGE_SIZE} total={total} hrefFor={(p) => p === 0 ? "/activity" : `/activity?page=${p}`} />
        </div>
      </div>
    </div>
  );
}
