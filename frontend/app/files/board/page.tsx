import { Ico } from "@/components/icons";
import { Av, AvStack, Ft, Pill, Tag } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFiles, safeOrgs, safeSearch, safeStats, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtBytes, parseJsonArray } from "@/lib/format";
import { canMutate } from "@/lib/roles";

import { ViewFilterBar } from "../view-filter-bar";
import { ViewTabs } from "../view-tabs";
import { fileMatchesFilters, readViewParams } from "../view-params";

const columns: Array<{ title: string; tone: "slate" | "amber" | "emerald"; accent: string }> = [
  { title: "Inbox",    tone: "slate",   accent: "#475569" },
  { title: "Review",   tone: "amber",   accent: "#d97706" },
  { title: "Approved", tone: "emerald", accent: "#059669" },
  { title: "Archived", tone: "slate",   accent: "#94a3b8" },
];

const statusToCol: Record<string, string> = {
  Draft: "Inbox", Review: "Review", Approved: "Approved", Archived: "Archived", Active: "Approved",
};

type BoardProps = { searchParams?: Promise<Record<string, string | string[] | undefined>> };

export default async function FilesBoardPage({ searchParams }: BoardProps) {
  const sp = (await searchParams) ?? {};
  const filters: Record<string, string> = {};
  for (const k of ["system_id", "org_id", "status", "project", "owner"] as const) {
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
  // Compose search + filters (and the backend-ignored owner filter) client-side.
  const rows = files.filter((f) => fileMatchesFilters(f, filters));
  const sys = filters.system_id ? systems.find((s) => s.id === filters.system_id) : undefined;
  const orgs = (sys ?? systems[0]) ? await safeOrgs((sys ?? systems[0]).id, cookieHeader) : [];
  const title = sys ? `${sys.name} board` : "All files";

  const counts: Record<string, number> = { Inbox: 0, Review: 0, Approved: 0, Archived: 0 };
  const grouped: Record<string, typeof rows> = { Inbox: [], Review: [], Approved: [], Archived: [] };
  for (const f of rows) {
    const col = statusToCol[f.status] ?? "Inbox";
    grouped[col].push(f);
    counts[col]++;
  }

  return (
    <div className="scr">
      <Sidebar nav="files" systems={systems} stats={stats} orgs={orgs} systemActive={filters.system_id ?? systems[0]?.id} />
      <TopBar
        crumbs={sys ? ["Workspace", sys.name, "Board"] : ["Workspace", "Board"]}
      />
      <div className="main">
        <div style={{ padding: "14px 24px 8px", borderBottom: "1px solid var(--border)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12 }}>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span className="t-2xl t-semibold">{title}</span>
                {sys && <Pill tone={sys.tone}><span className="dot" />{sys.name}</Pill>}
                <Pill>{rows.length} file{rows.length === 1 ? "" : "s"}</Pill>
              </div>
              <div className="t-sm t-muted" style={{ marginTop: 2 }}>
                Grouped by <span className="t-semibold" style={{ color: "var(--text)" }}>status</span> · 4 columns
              </div>
            </div>
            {canMutate(role) && (
              <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                <a className="btn primary" href="/upload"><Ico.upload /> Upload</a>
              </div>
            )}
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
            <ViewTabs params={viewParams} active="board" />
            <ViewFilterBar params={viewParams} base="/files/board" />
          </div>
        </div>

        <div style={{ flex: 1, overflow: "auto", padding: "16px 20px", background: "var(--bg-subtle)" }}>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(4, minmax(260px, 1fr))", gap: 16 }}>
            {columns.map((c) => (
              <div key={c.title} style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <span style={{ width: 10, height: 10, borderRadius: 3, background: c.accent }} />
                  <span className="t-md t-semibold">{c.title}</span>
                  <span className="t-sm t-muted t-tabular">{counts[c.title]}</span>
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                  {grouped[c.title].length === 0 ? (
                    <div className="t-xs t-subtle" style={{ padding: "8px 4px" }}>No files</div>
                  ) : grouped[c.title].slice(0, 6).map((f) => {
                    const tags = parseJsonArray(f.tags);
                    const colSys = systems.find((s) => s.id === f.system_id);
                    return (
                      <a key={f.id} href={`/files/${f.id}`} className="card" style={{ padding: 12, cursor: "pointer", color: "inherit", textDecoration: "none" }}>
                        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                          <Ft type={f.file_type} />
                          <div className="t-base t-semibold t-trunc" style={{ flex: 1, minWidth: 0 }}>{f.name}</div>
                        </div>
                        <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginBottom: 8 }}>
                          {colSys && <Pill tone={colSys.tone} sm><span className="dot" />{colSys.name}</Pill>}
                          {tags.slice(0, 3).map((t) => <Tag key={t}>{t}</Tag>)}
                        </div>
                        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", borderTop: "1px solid var(--border-subtle)", paddingTop: 8 }}>
                          <AvStack users={[f.owner]} />
                          <span className="t-xs t-mono t-muted">{f.file_type === "fold" ? "—" : fmtBytes(f.size_bytes)}</span>
                        </div>
                      </a>
                    );
                  })}
                  {grouped[c.title].length > 6 && (
                    <div className="t-xs t-subtle" style={{ padding: "4px 8px" }}>+ {grouped[c.title].length - 6} more</div>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
