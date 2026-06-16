import { Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import {
  safeRotationPolicies, safeRotationRuns, safeStats, safeSystems,
  safeSystemsWithPersonal, safeWorkspaceConfig,
} from "@/lib/api";
import { fmtBytes, fmtCount } from "@/lib/format";

import { GeneralForm }   from "./general-form";
import { loadSettingsCtx } from "./_shared";
import { SettingsNav }   from "./nav";
import { RotationPanel } from "./rotation-panel";
import { SystemsPanel }  from "./systems-panel";

/// /settings — Workspace General tab.  Everything visible here is wired to
/// a real source: workspace identity → workspace_config, storage policy →
/// /api/stats, buckets → /api/systems, rotation → /api/rotation/*.
/// The dimmed left-rail items ("Custom fields", etc.) are honest "Coming
/// soon" labels rather than fake controls.
export default async function SettingsGeneralPage() {
  const { cookieHeader, role } = await loadSettingsCtx();
  const isAdmin = role === "admin";

  const [systems, systemsAll, stats, policies, runs, cfg] = await Promise.all([
    safeSystems(cookieHeader),
    safeSystemsWithPersonal(cookieHeader),
    safeStats(cookieHeader),
    safeRotationPolicies(cookieHeader),
    safeRotationRuns(cookieHeader),
    safeWorkspaceConfig(cookieHeader),
  ]);

  const usedBytes  = stats?.total_size_bytes  ?? 0;
  const quotaBytes = stats?.total_quota_bytes ?? 0;
  const freeBytes  = Math.max(0, quotaBytes - usedBytes);
  const quotaPct   = quotaBytes > 0 ? Math.min(100, Math.round((usedBytes / quotaBytes) * 100)) : 0;

  const backendLabel = stats?.storage_backend ?? "";
  const backendKind  = backendLabel.startsWith("s3(") ? "s3" : "filesystem";
  const backendDetail = (() => {
    const m = backendLabel.match(/^[a-z0-9]+\((.+)\)$/i);
    return m?.[1] ?? backendLabel;
  })();

  return (
    <div className="scr">
      <Sidebar nav="settings" systems={systems} />
      <TopBar crumbs={["Settings"]} title="Workspace settings" />
      <div className="main split-rail" style={{ overflow: "auto" }}>
        <SettingsNav active="general" />

        <div className="main-pad" style={{ overflow: "auto" }}>
          <div style={{ maxWidth: 760 }}>
            <div className="t-3xl t-semibold">General</div>
            <div className="t-sm t-muted" style={{ marginTop: 4, marginBottom: 24 }}>
              Workspace identity, access policy, and defaults
            </div>

            <GeneralForm initial={cfg} canMutate={isAdmin} />

            <div className="card" style={{ padding: 20, marginBottom: 16 }}>
              <div className="t-md t-semibold" style={{ marginBottom: 12 }}>Storage policy</div>
              <div style={{ display: "grid", gridTemplateColumns: "180px 1fr", gap: "16px 24px", alignItems: "center" }}>
                <div className="t-sm t-muted">Primary backend</div>
                <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                  <Pill tone="emerald">{backendKind === "s3" ? "S3-compatible" : "Local filesystem"}</Pill>
                  <span className="t-sm t-mono t-subtle">{backendDetail || "—"}</span>
                  {stats && (
                    <Pill tone="emerald" sm>
                      <span className="dot" />
                      {stats.encryption_enabled ? "AES-256-GCM" : "plaintext"}
                    </Pill>
                  )}
                </div>

                <div className="t-sm t-muted">Quota</div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  {quotaBytes > 0 ? (
                    <>
                      <div className="prog" style={{ height: 6 }}>
                        <div className="bar" style={{
                          width: `${quotaPct}%`,
                          background: quotaPct >= 90 ? "var(--c-rose)" : quotaPct >= 70 ? "var(--c-amber)" : "var(--c-emerald)",
                        }} />
                      </div>
                      <div className="t-xs t-muted" style={{ marginTop: 4 }}>
                        {fmtBytes(usedBytes)} of {fmtBytes(quotaBytes)} used · {fmtBytes(freeBytes)} free · {quotaPct}%
                      </div>
                    </>
                  ) : (
                    <div className="t-xs t-muted">
                      No quota set — {fmtBytes(usedBytes)} used.
                    </div>
                  )}
                </div>

                <div className="t-sm t-muted">Files in workspace</div>
                <div className="t-sm">
                  <span className="t-tabular t-semibold">{stats ? fmtCount(stats.total_files) : "—"}</span>
                  <span className="t-muted"> · across {stats ? fmtCount(stats.connected_systems.length) : "—"} system{stats?.connected_systems.length === 1 ? "" : "s"}</span>
                </div>

                <div className="t-sm t-muted">Org records</div>
                <div className="t-sm">
                  <span className="t-tabular t-semibold">{stats ? fmtCount(stats.total_orgs) : "—"}</span>
                  <span className="t-muted"> · {stats ? fmtCount(stats.active_orgs) : "—"} active</span>
                </div>
              </div>
            </div>

            <SystemsPanel initial={systemsAll.filter((s) => s.system_type === "shared")} canMutate={isAdmin} />

            <RotationPanel initialPolicies={policies} initialRuns={runs} systems={systemsAll} canMutate={isAdmin} />

            <div className="card" style={{ padding: 20, borderColor: "var(--danger-border)" }}>
              <div className="t-md t-semibold" style={{ marginBottom: 4, color: "var(--danger)" }}>Danger zone</div>
              <div className="t-xs t-muted" style={{ marginBottom: 14 }}>These actions cannot be undone</div>
              <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <div style={{ flex: 1 }}>
                    <div className="t-sm t-semibold">Disconnect a storage backend</div>
                    <div className="t-xs t-muted">Move files to a different bucket first, then delete from the Storage buckets table above.</div>
                  </div>
                </div>
                <div style={{ display: "flex", alignItems: "center", gap: 10, paddingTop: 10, borderTop: "1px solid var(--border)" }}>
                  <div style={{ flex: 1 }}>
                    <div className="t-sm t-semibold">Delete workspace</div>
                    <div className="t-xs t-muted">
                      Permanently delete all{" "}
                      <span className="t-tabular t-semibold">{stats ? fmtCount(stats.total_files) : "—"}</span> file{stats?.total_files === 1 ? "" : "s"} and{" "}
                      <span className="t-tabular t-semibold">{stats ? fmtCount(stats.total_orgs) : "—"}</span> org record{stats?.total_orgs === 1 ? "" : "s"}
                    </div>
                  </div>
                  <button className="btn sm danger" disabled title="Deleting a workspace requires a system administrator.">Contact admin</button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
