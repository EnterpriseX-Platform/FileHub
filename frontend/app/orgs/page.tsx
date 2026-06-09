import * as React from "react";

import { Ico } from "@/components/icons";
import { Av, Pill, Tag, type Tone } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFiles, safeOrgs, safeStats, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtBytes, parseJsonArray, statusTone } from "@/lib/format";
import { canMutate } from "@/lib/roles";

const tierToneMap: Record<string, Tone> = {
  HQ: "amber", Department: "rose", Section: "cyan", Group: "cyan",
};

type OrgsProps = { searchParams?: Promise<Record<string, string | string[] | undefined>> };

export default async function OrgsPage({ searchParams }: OrgsProps) {
  const sp = (await searchParams) ?? {};
  const systemId = typeof sp.system_id === "string" ? sp.system_id : undefined;

  const { cookieHeader, role } = await loadServerCtx();
  const [systems, stats] = await Promise.all([safeSystems(cookieHeader), safeStats(cookieHeader)]);
  const activeSystemId = systemId ?? systems[0]?.id;
  const [orgs, allFiles] = await Promise.all([
    activeSystemId ? safeOrgs(activeSystemId, cookieHeader) : Promise.resolve([]),
    activeSystemId ? safeFiles({ system_id: activeSystemId, limit: "2000" }, cookieHeader) : Promise.resolve([]),
  ]);

  // Compute real per-org file counts + storage from the file list. This lets
  // us drop the random-number placeholders the prototype used.
  const filesByOrg: Record<string, { count: number; size: number; latest: string }> = {};
  for (const f of allFiles) {
    if (!f.org_id) continue;
    const bucket = filesByOrg[f.org_id] ??= { count: 0, size: 0, latest: "" };
    bucket.count += 1;
    bucket.size  += f.size_bytes;
    if (!bucket.latest || f.modified_at > bucket.latest) bucket.latest = f.modified_at;
  }

  const totalOrgsAcrossSystems = systems.reduce((s, _) => s + 0, 0); // placeholder — we don't have a global count endpoint
  // Show the count for the loaded system instead.
  const headerCount = orgs.length;

  const activeSystem = systems.find((s) => s.id === activeSystemId);

  return (
    <div className="scr">
      <Sidebar nav="settings" systems={systems} stats={stats} orgs={orgs} systemActive={activeSystemId} />
      <TopBar
        crumbs={["Workspace", "Orgs & Systems"]}
        title="Orgs & Systems"
        actions={
          <>
            {canMutate(role) && <a className="btn" href="/upload"><Ico.upload /> Upload</a>}
          </>
        }
      />
      <div className="main">
        <div className="main-pad" style={{ paddingBottom: 0, display: "flex", alignItems: "flex-end", gap: 16, borderBottom: "1px solid var(--border)" }}>
          <div>
            <div className="t-2xl t-semibold">Orgs & Systems</div>
            <div className="t-sm t-muted" style={{ marginTop: 2 }}>
              {systems.length} system{systems.length === 1 ? "" : "s"}
              {activeSystem && <> · viewing {activeSystem.name} ({headerCount} org{headerCount === 1 ? "" : "s"})</>}
            </div>
          </div>
        </div>

        <div className="split-rail" style={{ flex: 1, overflow: "auto", "--rail": "300px" } as React.CSSProperties}>
          <div style={{ borderRight: "1px solid var(--border)", padding: "12px 8px", background: "var(--bg-subtle)", overflow: "auto" }}>
            <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", padding: "4px 10px 8px" }}>
              Systems
            </div>
            {systems.map((sys) => {
              const active = sys.id === activeSystemId;
              const fileCount = stats?.connected_systems.find((c) => c.id === sys.id)?.file_count ?? 0;
              return (
                <a
                  key={sys.id}
                  href={`/orgs?system_id=${sys.id}`}
                  className={"side-row" + (active ? " active" : "")}
                  style={{ padding: "8px 10px", display: "block" }}
                >
                  <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <Pill tone={sys.tone}><span className="dot" /></Pill>
                    <span className="t-base t-semibold" style={{ flex: 1, color: "var(--text)" }}>{sys.name}</span>
                    <span style={{ width: 6, height: 6, borderRadius: 3, background: sys.status === "live" ? "var(--success)" : "var(--text-faint)" }} />
                  </div>
                  <div className="t-xs t-muted" style={{ marginTop: 4, paddingLeft: 28 }}>
                    {fileCount} file{fileCount === 1 ? "" : "s"} · bucket {sys.bucket}
                  </div>
                </a>
              );
            })}
          </div>

          <div style={{ overflow: "auto" }}>
            <div className="table-scroll">
            <table className="tbl">
              <thead>
                <tr>
                  <th>Organization</th>
                  <th style={{ width: 110 }}>Code</th>
                  <th style={{ width: 110 }}>Tier</th>
                  <th style={{ width: 90, textAlign: "right" }}>Files</th>
                  <th style={{ width: 110, textAlign: "right" }}>Storage</th>
                  <th style={{ width: 160 }}>Owner</th>
                  <th>Tags</th>
                  <th style={{ width: 100 }}>Status</th>
                </tr>
              </thead>
              <tbody>
                {orgs.length === 0 && (
                  <tr><td colSpan={8} style={{ padding: 40, textAlign: "center", color: "var(--text-subtle)" }}>
                    {activeSystem ? "No organizations in this system yet — set up org hierarchies in Settings to organize teams and permissions." : "No orgs registered."}
                  </td></tr>
                )}
                {orgs.map((o) => {
                  const tags = parseJsonArray(o.tags);
                  const tierTone = tierToneMap[o.tier] ?? "slate";
                  const bucket = filesByOrg[o.id];
                  return (
                    <tr key={o.id}>
                      <td>
                        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                          <Ico.bucket className="icon sm" style={{ color: "var(--text-muted)" }} />
                          <a href={`/files?system_id=${o.system_id}&org_id=${o.id}`} className="t-medium" style={{ color: "var(--text)" }}>{o.name}</a>
                        </div>
                      </td>
                      <td><span className="t-mono t-xs t-muted">{o.code}</span></td>
                      <td><Pill tone={tierTone}>{o.tier}</Pill></td>
                      <td className="t-mono t-sm t-tabular t-muted" style={{ textAlign: "right" }}>{bucket?.count ?? 0}</td>
                      <td className="t-mono t-sm t-tabular t-muted" style={{ textAlign: "right" }}>{bucket ? fmtBytes(bucket.size) : "—"}</td>
                      <td>
                        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                          <Av name={o.owner} /><span className="t-sm">{o.owner}</span>
                        </div>
                      </td>
                      <td>
                        <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
                          {tags.map((t) => <Tag key={t}>{t}</Tag>)}
                        </div>
                      </td>
                      <td><Pill tone={statusTone(o.status)}><span className="dot" />{o.status}</Pill></td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            </div>

            <div style={{ padding: "12px 16px", borderTop: "1px solid var(--border)" }}>
              <div className="t-sm t-muted">{orgs.length} org{orgs.length === 1 ? "" : "s"} in {activeSystem?.name ?? "—"}</div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
