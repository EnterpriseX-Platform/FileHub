import { Ico } from "@/components/icons";
import { Pill, SectionHd } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { Ft } from "@/components/primitives";
import { UserGreeting } from "@/components/user-greeting";
import { safeStats, safeActivity, safeSystems, safeFiles, safeViews, safeOrgs } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtBytes, fmtCount } from "@/lib/format";
import { canMutate } from "@/lib/roles";
import { hrefForView } from "@/lib/view-href";
import Link from "next/link";

import { DashboardActivity } from "./dashboard-activity";
import { SyncButton } from "./sync-button";

export default async function DashboardPage() {
  // Every safe* helper below hits a private /fh/api/* route, so the inbound
  // session cookie must be forwarded or the backend 401s and the dashboard
  // renders "Backend offline" with "—" in every stat card.
  const { cookieHeader, role } = await loadServerCtx();
  const [stats, activity, systems, reviewFiles, views] = await Promise.all([
    safeStats(cookieHeader),
    safeActivity(8, cookieHeader),
    safeSystems(cookieHeader),
    safeFiles({ status: "Review", limit: "5" }, cookieHeader),
    safeViews(cookieHeader),
  ]);
  const orgs = systems[0] ? await safeOrgs(systems[0].id, cookieHeader) : [];

  const reviewSystemsCount = new Set(reviewFiles.map((f) => f.system_id)).size;
  const orgsWithReview     = new Set(reviewFiles.map((f) => f.org_id).filter(Boolean)).size;

  // Storage quota comes from the workspace_config table — admins can adjust
  // it without a redeploy. Fall back to 0 only if the row is missing.
  const quotaBytes = stats ? stats.total_quota_bytes : 0;
  const usedBytes  = stats ? stats.total_size_bytes  : 0;
  const quotaPct = quotaBytes > 0 ? Math.min(100, Math.round((usedBytes / quotaBytes) * 100)) : 0;

  const cards: Array<[string, string, string, "indigo" | "emerald" | "amber" | "rose", React.ReactNode]> = [
    [
      "Total files",
      stats ? fmtCount(stats.total_files) : "—",
      stats ? `${fmtBytes(stats.total_size_bytes)} on disk` : "backend offline",
      "indigo",
      <Ico.files />,
    ],
    [
      "Storage used",
      stats ? fmtBytes(usedBytes) : "—",
      quotaBytes > 0 ? `${quotaPct}% of ${fmtBytes(quotaBytes)} quota` : "no quota set",
      "emerald",
      <Ico.database />,
    ],
    [
      "Active orgs",
      stats ? fmtCount(stats.active_orgs) : "—",
      `${(stats?.connected_systems ?? []).length} systems connected`,
      "amber",
      <Ico.users />,
    ],
    [
      "Awaiting review",
      stats ? fmtCount(stats.awaiting_review) : "—",
      orgsWithReview ? `across ${orgsWithReview} org${orgsWithReview === 1 ? "" : "s"}` : "no pending reviews",
      "rose",
      <Ico.warning />,
    ],
  ];

  const storageBySystem = stats?.storage_by_system ?? [];
  const totalStorage = storageBySystem.reduce((s, r) => s + r.size_bytes, 0) || 1;

  const pinnedViews = views.filter((v) => Boolean(v.pinned)).slice(0, 4);

  return (
    <div className="scr">
      <Sidebar nav="dashboard" systems={systems} stats={stats} orgs={orgs} />
      <TopBar
        crumbs={["Workspace", "Dashboard"]}
        actions={
          <>
            {canMutate(role) && <a className="btn" href="/upload"><Ico.upload /> Upload</a>}
          </>
        }
      />
      <div className="main main-pad" style={{ overflow: "auto" }}>
        <div className="page">
        <div style={{ display: "flex", alignItems: "flex-end", gap: 16, marginBottom: 24, flexWrap: "wrap" }}>
          <div>
            <UserGreeting />
            <div className="t-md t-muted" style={{ marginTop: 4 }}>
              {stats?.awaiting_review ? (
                <>
                  You have <span className="t-semibold" style={{ color: "var(--text)" }}>{stats.awaiting_review} file{stats.awaiting_review === 1 ? "" : "s"} awaiting review</span>
                  {reviewSystemsCount > 0 && <> across {reviewSystemsCount} system{reviewSystemsCount === 1 ? "" : "s"}</>}.
                </>
              ) : stats ? (
                <>Your inbox is clear — no files awaiting review.</>
              ) : (
                <>Backend offline — start the Rust API on :8090 to load real data.</>
              )}
            </div>
          </div>
          <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
            <SyncButton />
            {canMutate(role) && <a className="btn primary" href="/upload"><Ico.upload /> Quick upload</a>}
          </div>
        </div>

        <div className="stat-cards" style={{ marginBottom: 24 }}>
          {cards.map(([label, value, hint, tone, icon], i) => (
            <div key={i} className="card" style={{ padding: 16 }}>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 10 }}>
                <div className="t-sm t-muted">{label}</div>
                <Pill tone={tone}>{icon}</Pill>
              </div>
              <div className="t-3xl t-semibold t-tabular" style={{ lineHeight: 1 }}>{value}</div>
              <div className="t-xs t-muted" style={{ marginTop: 8 }}>{hint}</div>
            </div>
          ))}
        </div>

        <div className="content-grid-2col">
          <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            <div className="card" style={{ padding: 16 }}>
              <SectionHd
                title="Storage by system"
                sub={`${fmtBytes(totalStorage)} used`}
                action={<a className="btn sm ghost" href="/orgs">View details <Ico.chevron className="icon sm" /></a>}
              />
              {storageBySystem.length === 0 ? (
                <div className="t-sm t-subtle" style={{ padding: "12px 0" }}>Upload a file to see storage usage by system.</div>
              ) : (
                <>
                  <div style={{ display: "flex", height: 10, borderRadius: 5, overflow: "hidden", marginBottom: 12 }}>
                    {storageBySystem.map((r) => (
                      <div
                        key={r.system_id}
                        style={{ flex: r.size_bytes / totalStorage, background: `var(--c-${r.tone})` }}
                        title={`${r.name} · ${fmtBytes(r.size_bytes)}`}
                      />
                    ))}
                  </div>
                  <div className="legend-grid">
                    {storageBySystem.map((r) => (
                      <div key={r.system_id} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                        <span style={{ width: 8, height: 8, borderRadius: 2, background: `var(--c-${r.tone})`, flexShrink: 0 }} />
                        <span className="t-base t-medium" style={{ flex: 1, minWidth: 0 }}>{r.name}</span>
                        <span className="t-sm t-mono t-muted">{fmtBytes(r.size_bytes)}</span>
                        <span className="t-sm t-mono t-faint" style={{ width: 32, textAlign: "right" }}>
                          {Math.round((r.size_bytes / totalStorage) * 100)}%
                        </span>
                      </div>
                    ))}
                  </div>
                </>
              )}
            </div>

            <div className="card" style={{ padding: 0 }}>
              <DashboardActivity activity={activity} />
            </div>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            <div className="card" style={{ padding: 16 }}>
              <SectionHd
                title="Awaiting your review"
                sub={`${reviewFiles.length} item${reviewFiles.length === 1 ? "" : "s"}`}
                action={<a className="btn xs ghost" href="/files?status=Review">View all <Ico.chevron className="icon sm" /></a>}
              />
              {reviewFiles.length === 0 ? (
                <div className="t-sm t-subtle" style={{ padding: "8px 0" }}>Your inbox is clear — no files waiting for your review.</div>
              ) : reviewFiles.map((f, i) => {
                const sys = systems.find((s) => s.id === f.system_id);
                return (
                  <div key={f.id} style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 0", borderTop: i ? "1px solid var(--border)" : "none" }}>
                    <Ft type={f.file_type} />
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <a href={`/files/${f.id}`} className="t-base t-medium t-trunc" style={{ color: "var(--text)", display: "block" }}>{f.name}</a>
                      <div className="t-xs t-muted t-trunc">{sys?.name ?? f.system_id} · {f.owner}</div>
                    </div>
                    <a className="btn xs primary" href={`/files/${f.id}`}>Review</a>
                  </div>
                );
              })}
            </div>

            <div className="card" style={{ padding: 16 }}>
              <SectionHd title="Pinned views" action={<Ico.pin className="icon sm" />} />
              {pinnedViews.length === 0 ? (
                <div className="t-sm t-subtle" style={{ padding: "8px 0" }}>No pinned views yet. Save a filter in Files, then pin it here.</div>
              ) : pinnedViews.map((v, i) => {
                const icon = v.layout === "board" ? <Ico.board /> : v.layout === "gallery" ? <Ico.gallery /> : <Ico.table />;
                const tone: "indigo" | "rose" | "emerald" | "amber" =
                  v.layout === "board"   ? "indigo" :
                  v.layout === "gallery" ? "amber"  :
                  v.color === "#dc2626"  ? "rose"   :
                  v.color === "#16a34a"  ? "emerald": "indigo";
                // Q8 — each pinned view now navigates somewhere useful:
                // derive a /files?<filter>=<value> URL from the view's first
                // equality filter (see lib/view-href.ts).
                return (
                  <Link
                    key={v.id}
                    href={hrefForView(v.filters)}
                    style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 0", borderTop: i ? "1px solid var(--border)" : "none", textDecoration: "none", color: "inherit" }}
                  >
                    <span className={"pill " + tone} style={{ width: 26, height: 26, padding: 0, justifyContent: "center", borderRadius: 6 }}>{icon}</span>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div className="t-base t-medium t-trunc">{v.name}</div>
                      <div className="t-xs t-muted t-trunc">{v.layout} view</div>
                    </div>
                    <Ico.chevron className="icon sm" style={{ color: "var(--text-subtle)" }} />
                  </Link>
                );
              })}
            </div>

            <div className="card" style={{ padding: 16 }}>
              <SectionHd
                title="Connected systems"
                sub={`${(stats?.connected_systems ?? []).length} system${(stats?.connected_systems ?? []).length === 1 ? "" : "s"} syncing`}
                action={<a className="btn xs" href="/orgs">Manage</a>}
              />
              {(stats?.connected_systems ?? []).slice(0, 4).map((s, i) => (
                <div key={s.id} style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 0", borderTop: i ? "1px solid var(--border)" : "none" }}>
                  <Pill tone={s.tone}><span className="dot" />{s.name}</Pill>
                  <div className="t-xs t-muted" style={{ marginLeft: "auto" }}>{s.file_count} file{s.file_count === 1 ? "" : "s"}</div>
                  <span style={{ width: 6, height: 6, borderRadius: 3, background: s.status === "live" ? "var(--success)" : "var(--text-subtle)" }} />
                </div>
              ))}
              {(stats?.connected_systems ?? []).length === 0 && (
                <div className="t-sm t-subtle" style={{ padding: "8px 0" }}>No storage systems connected. Admins can add one in Settings.</div>
              )}
            </div>
          </div>
        </div>
        </div>
      </div>
    </div>
  );
}
