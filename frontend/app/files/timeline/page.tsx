import { Ico } from "@/components/icons";
import { Ft, Pill, Tag } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFiles, safeOrgs, safeSearch, safeStats, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtBytes, parseJsonArray, statusTone } from "@/lib/format";
import { canMutate } from "@/lib/roles";
import type { FileRow } from "@/lib/api";

import { ViewFilterBar } from "../view-filter-bar";
import { ViewTabs } from "../view-tabs";
import { fileMatchesFilters, readViewParams } from "../view-params";

type TimelineProps = { searchParams?: Promise<Record<string, string | string[] | undefined>> };

function dateLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date(); today.setHours(0, 0, 0, 0);
  const that = new Date(d);  that.setHours(0, 0, 0, 0);
  const days = Math.round((today.getTime() - that.getTime()) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7)   return d.toLocaleDateString("en-US", { weekday: "long" });
  return d.toLocaleDateString("en-US", { day: "numeric", month: "long", year: "numeric" });
}

function dateKey(iso: string): string {
  return iso.slice(0, 10);
}

function timeOnly(iso: string): string {
  return new Date(iso).toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false });
}

export default async function FilesTimelinePage({ searchParams }: TimelineProps) {
  const sp = (await searchParams) ?? {};
  const filters: Record<string, string> = {};
  for (const k of ["system_id", "org_id", "status", "project", "owner", "folder_id"] as const) {
    const v = sp[k];
    if (typeof v === "string" && v.length) filters[k] = v;
  }
  const searchTerm = typeof sp.q === "string" ? sp.q.trim() : "";
  const viewParams = readViewParams(sp);
  const { cookieHeader, role } = await loadServerCtx();
  const [files, systems, stats] = await Promise.all([
    searchTerm ? safeSearch(searchTerm, cookieHeader) : safeFiles(filters, cookieHeader),
    safeSystems(cookieHeader),
    safeStats(cookieHeader),
  ]);
  const rows = files.filter((f) => fileMatchesFilters(f, filters));
  const sys = filters.system_id ? systems.find((s) => s.id === filters.system_id) : undefined;
  const orgs = (sys ?? systems[0]) ? await safeOrgs((sys ?? systems[0]).id, cookieHeader) : [];

  // Group by day. Files are already sorted modified_at DESC by the API.
  const groups: Array<{ key: string; label: string; files: FileRow[] }> = [];
  for (const f of rows) {
    const k = dateKey(f.modified_at);
    let g = groups.find((x) => x.key === k);
    if (!g) {
      g = { key: k, label: dateLabel(f.modified_at), files: [] };
      groups.push(g);
    }
    g.files.push(f);
  }

  return (
    <div className="scr">
      <Sidebar nav="files" systems={systems} stats={stats} orgs={orgs} systemActive={filters.system_id ?? systems[0]?.id} />
      <TopBar
        crumbs={sys ? ["Workspace", sys.name, "Timeline"] : ["Workspace", "Timeline"]}
        actions={canMutate(role) ? <a className="btn" href="/upload"><Ico.upload /> Upload</a> : undefined}
      />
      <div className="main">
        <div style={{ padding: "14px 24px 8px", borderBottom: "1px solid var(--border)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
            <ViewTabs params={viewParams} active="timeline" />
            <ViewFilterBar params={viewParams} base="/files/timeline" />
          </div>
        </div>

        <div style={{ padding: "16px 24px", borderBottom: "1px solid var(--border)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span className="t-2xl t-semibold">{sys ? `${sys.name} timeline` : "Timeline"}</span>
            {sys && <Pill tone={sys.tone}><span className="dot" />{sys.name}</Pill>}
            <Pill>{rows.length} file{rows.length === 1 ? "" : "s"}</Pill>
          </div>
          <div className="t-sm t-muted" style={{ marginTop: 2 }}>Sorted by last-modified · grouped by day</div>
        </div>

        <div style={{ flex: 1, overflow: "auto", padding: "20px 24px" }}>
          {groups.length === 0 && (
            <div style={{ padding: 40, textAlign: "center", color: "var(--text-subtle)" }}>
              No files match this view.{canMutate(role) && <> <a href="/upload" style={{ color: "var(--accent)" }}>Upload a file</a>.</>}
            </div>
          )}
          {groups.map((g) => (
            <div key={g.key} style={{ marginBottom: 28 }}>
              <div style={{ display: "flex", alignItems: "baseline", gap: 12, marginBottom: 10 }}>
                <span className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase" }}>{g.label}</span>
                <span className="t-xs t-mono t-subtle">{g.key}</span>
                <span className="t-xs t-subtle" style={{ marginLeft: "auto" }}>{g.files.length} file{g.files.length === 1 ? "" : "s"}</span>
              </div>
              <div style={{ position: "relative", paddingLeft: 22 }}>
                <div style={{ position: "absolute", left: 8, top: 4, bottom: 4, width: 1, background: "var(--border)" }} />
                {g.files.map((f) => {
                  const fileSys = systems.find((s) => s.id === f.system_id);
                  const tags = parseJsonArray(f.tags);
                  return (
                    <div key={f.id} style={{ position: "relative", display: "flex", alignItems: "center", gap: 10, padding: "8px 0" }}>
                      <span style={{
                        position: "absolute", left: -19, top: 16, width: 8, height: 8, borderRadius: 4,
                        background: `var(--c-${fileSys?.tone ?? "slate"})`,
                      }} />
                      <span className="t-xs t-mono t-subtle" style={{ width: 44 }}>{timeOnly(f.modified_at)}</span>
                      <Ft type={f.file_type} />
                      <a href={`/files/${f.id}`} style={{ color: "var(--text)", textDecoration: "none", flex: 1, minWidth: 0 }}>
                        <div className="t-base t-medium t-trunc">{f.name}</div>
                        <div className="t-xs t-muted t-trunc">{f.owner} · v{f.version}</div>
                      </a>
                      <Pill tone={statusTone(f.status)} sm><span className="dot" />{f.status}</Pill>
                      {fileSys && <Pill tone={fileSys.tone} sm><span className="dot" />{fileSys.name}</Pill>}
                      {tags.slice(0, 2).map((t) => <Tag key={t}>{t}</Tag>)}
                      <span className="t-xs t-mono t-muted" style={{ width: 70, textAlign: "right" }}>
                        {f.file_type === "fold" ? "—" : fmtBytes(f.size_bytes)}
                      </span>
                    </div>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

