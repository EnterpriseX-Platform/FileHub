import { cookies } from "next/headers";
import { redirect } from "next/navigation";

import { CountUp } from "@/components/count-up";
import { Ico } from "@/components/icons";
import { SectionHd } from "@/components/primitives";
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

/// SVG donut of storage share per system (server-rendered — no client JS).
/// Stroke colors come from the tonal CSS variables so dark mode follows.
function StorageDonut({ rows, total }: {
  rows: Array<{ system_id: string; name: string; tone: string; size_bytes: number }>;
  total: number;
}) {
  const R = 40;
  const C = 2 * Math.PI * R;
  let acc = 0;
  return (
    <div style={{ position: "relative", width: 128, height: 128, flexShrink: 0 }}>
      <svg viewBox="0 0 100 100" width={128} height={128} role="img" aria-label="Storage by system">
        <circle cx="50" cy="50" r={R} fill="none" stroke="var(--bg-muted)" strokeWidth="11" />
        {rows.map((r) => {
          const frac = r.size_bytes / total;
          const start = acc;
          acc += frac;
          // Tiny gap between segments so adjacent tones don't merge.
          const dash = Math.max(0, frac * C - 1.5);
          return (
            <circle
              key={r.system_id}
              cx="50" cy="50" r={R} fill="none"
              stroke={`var(--c-${r.tone})`} strokeWidth="11"
              strokeDasharray={`${dash} ${C - dash}`}
              strokeDashoffset={-start * C}
              transform="rotate(-90 50 50)"
            />
          );
        })}
      </svg>
      <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center" }}>
        <span className="t-sm t-semibold t-tabular">{fmtBytes(total)}</span>
        <span className="t-xs t-subtle">used</span>
      </div>
    </div>
  );
}

export default async function DashboardPage() {
  // Every safe* helper below hits a private /fh/api/* route, so the inbound
  // session cookie must be forwarded or the backend 401s and the dashboard
  // renders "Backend offline" with "—" in every stat card.
  const { cookieHeader, role } = await loadServerCtx();

  // `/` is the Workspace (power) home. Everyday-mode users belong in the
  // simplified app — bounce them to /home. Mode = the fh-view cookie, or a
  // role default when unset (admins → workspace, everyone else → everyday).
  const viewCookie = (await cookies()).get("fh-view")?.value;
  const mode = viewCookie ?? (role === "admin" ? "workspace" : "everyday");
  if (mode === "everyday") redirect("/home");
  const [stats, activity, systems, reviewFiles, views] = await Promise.all([
    safeStats(cookieHeader),
    safeActivity(8, cookieHeader),
    safeSystems(cookieHeader),
    safeFiles({ status: "Review", limit: "5" }, cookieHeader),
    safeViews(cookieHeader),
  ]);
  const orgs = systems[0] ? await safeOrgs(systems[0].id, cookieHeader) : [];

  const reviewSystemsCount = new Set(reviewFiles.map((f) => f.system_id)).size;

  // Storage quota comes from the workspace_config table — admins can adjust
  // it without a redeploy. Fall back to 0 only if the row is missing.
  const quotaBytes = stats ? stats.total_quota_bytes : 0;
  const usedBytes  = stats ? stats.total_size_bytes  : 0;
  const quotaPct = quotaBytes > 0 ? Math.min(100, Math.round((usedBytes / quotaBytes) * 100)) : 0;

  // One-line stats strip: [value, label] pairs — replaces four stat cards
  // whose numbers all reappeared in the panels below them.
  const strip: Array<[string, string]> = stats ? [
    [fmtCount(stats.total_files), "files"],
    [fmtBytes(usedBytes), quotaBytes > 0 ? `of ${fmtBytes(quotaBytes)} (${quotaPct}%)` : "on disk"],
    [fmtCount(stats.active_orgs), "orgs"],
  ] : [];

  const storageBySystem = stats?.storage_by_system ?? [];
  const totalStorage = storageBySystem.reduce((s, r) => s + r.size_bytes, 0) || 1;

  // Collapse the legend to the largest few systems + an "others" bucket so the
  // dashboard doesn't list 7 systems when 5 are at ~0%.
  const STORAGE_TOP = 4;
  const storageSorted = [...storageBySystem].sort((a, b) => b.size_bytes - a.size_bytes);
  const storageDisplay = storageSorted.length > STORAGE_TOP + 1
    ? [
        ...storageSorted.slice(0, STORAGE_TOP),
        {
          system_id: "__others__",
          name: `+${storageSorted.length - STORAGE_TOP} more`,
          tone: "slate",
          size_bytes: storageSorted.slice(STORAGE_TOP).reduce((s, r) => s + r.size_bytes, 0),
        },
      ]
    : storageSorted;

  const pinnedViews = views.filter((v) => Boolean(v.pinned)).slice(0, 4);

  return (
    <div className="scr">
      <Sidebar nav="dashboard" systems={systems} stats={stats} orgs={orgs} />
      <TopBar crumbs={["Workspace", "Dashboard"]} />
      <div className="main main-pad" style={{ overflow: "auto" }}>
        <div className="page">
        {/* Hero — greeting, one status sentence, ONE primary action, and a
            single-line stats strip (the old four stat cards all repeated
            numbers shown in the panels below). */}
        <div style={{ display: "flex", alignItems: "flex-end", gap: 16, marginBottom: 8, flexWrap: "wrap" }}>
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
                <>We couldn’t load your workspace data right now. Please refresh in a moment.</>
              )}
            </div>
          </div>
          <div style={{ marginLeft: "auto" }}>
            {canMutate(role) && <a className="btn primary" href="/upload"><Ico.upload /> Upload</a>}
          </div>
        </div>

        {strip.length > 0 && (
          <div className="t-sm t-muted t-tabular" style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "baseline", marginBottom: 28 }}>
            {strip.map(([value, label], i) => (
              <span key={i} style={{ display: "inline-flex", gap: 5, alignItems: "baseline" }}>
                {i > 0 && <span className="t-faint" style={{ marginRight: 8 }}>·</span>}
                <span className="t-semibold" style={{ color: "var(--text)" }}><CountUp value={value} /></span>
                <span>{label}</span>
              </span>
            ))}
          </div>
        )}

        {/* What needs you — the only actionable block, so it comes first,
            full-width. */}
        {reviewFiles.length > 0 && (
          <div className="card" style={{ padding: 16, marginBottom: 16 }}>
            <SectionHd
              title="Needs your review"
              sub={`${reviewFiles.length} item${reviewFiles.length === 1 ? "" : "s"}`}
              action={<Link className="btn xs ghost" href="/files?status=Review">View all <Ico.chevron className="icon sm" /></Link>}
            />
            {reviewFiles.map((f, i) => {
              const sys = systems.find((s) => s.id === f.system_id);
              return (
                <div key={f.id} style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 0", borderTop: i ? "1px solid var(--border-subtle)" : "none" }}>
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
        )}

        <div className="content-grid-2col">
          <div className="card" style={{ padding: 0, alignSelf: "start" }}>
            <DashboardActivity activity={activity} />
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            <div className="card" style={{ padding: 16 }}>
              <SectionHd
                title="Storage"
                sub={`${fmtBytes(totalStorage)} used`}
                action={<SyncButton />}
              />
              {storageBySystem.length === 0 ? (
                <div className="t-sm t-muted" style={{ padding: "12px 0" }}>Upload a file to see storage usage by system.</div>
              ) : (
                <div style={{ display: "flex", gap: 20, alignItems: "center", flexWrap: "wrap" }}>
                  <StorageDonut rows={storageDisplay} total={totalStorage} />
                  <div style={{ flex: 1, minWidth: 180, display: "flex", flexDirection: "column", gap: 6 }}>
                    {storageDisplay.map((r) => (
                      <div key={r.system_id} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                        <span style={{ width: 8, height: 8, borderRadius: 2, background: `var(--c-${r.tone})`, flexShrink: 0 }} />
                        <span className="t-sm t-medium t-trunc" style={{ flex: 1, minWidth: 0 }}>{r.name}</span>
                        <span className="t-xs t-tabular t-muted" style={{ flexShrink: 0, whiteSpace: "nowrap" }}>{fmtBytes(r.size_bytes)}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>

            {pinnedViews.length > 0 && (
              <div className="card" style={{ padding: 16 }}>
                <SectionHd title="Pinned views" action={<Ico.pin className="icon sm" />} />
                <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
                  {pinnedViews.map((v) => {
                    const icon = v.layout === "board" ? <Ico.board className="icon sm" /> : v.layout === "gallery" ? <Ico.gallery className="icon sm" /> : <Ico.table className="icon sm" />;
                    // Each pinned view navigates via its first equality filter
                    // (see lib/view-href.ts).
                    return (
                      <Link key={v.id} href={hrefForView(v.filters)} className="ask-sug" style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                        {icon} {v.name}
                      </Link>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        </div>
        </div>
      </div>
    </div>
  );
}
