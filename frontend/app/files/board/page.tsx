import Link from "next/link";

import { Ico } from "@/components/icons";
import { Av, AvStack, Ft, Pill, Tag } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFiles, safeOrgs, safeSearch, safeStats, safeSystems } from "@/lib/api";
import type { FileRow } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtBytes, parseJsonArray } from "@/lib/format";
import { canMutate } from "@/lib/roles";

import { GroupMenu } from "../table-toolbar";
import { ViewFilterBar } from "../view-filter-bar";
import { ViewTabs } from "../view-tabs";
import { buildViewHref, fileMatchesFilters, GROUP_FIELDS, readViewParams } from "../view-params";

// Status grouping (default) — fixed columns with tones, mapping the file's
// freeform status onto one of four lanes.
const STATUS_COLUMNS: Array<{ title: string; accent: string }> = [
  { title: "Inbox",    accent: "#475569" },
  { title: "Review",   accent: "#d97706" },
  { title: "Approved", accent: "#059669" },
  { title: "Archived", accent: "#94a3b8" },
];

const statusToCol: Record<string, string> = {
  Draft: "Inbox", Review: "Review", Approved: "Approved", Archived: "Archived", Active: "Approved",
};

// Accent palette cycled through for dynamically-derived columns (owner/project).
const DYNAMIC_ACCENTS = ["#475569", "#d97706", "#059669", "#7c3aed", "#0891b2", "#db2777", "#65a30d", "#dc2626"];

type BoardProps = { searchParams?: Promise<Record<string, string | string[] | undefined>> };

export default async function FilesBoardPage({ searchParams }: BoardProps) {
  const sp = (await searchParams) ?? {};
  const filters: Record<string, string> = {};
  for (const k of ["system_id", "org_id", "status", "project", "owner", "folder_id"] as const) {
    const v = sp[k];
    if (typeof v === "string" && v.length) filters[k] = v;
  }
  const searchTerm = typeof sp.q === "string" ? sp.q.trim() : "";
  const viewParams = readViewParams(sp);

  // Group field carried in ?group= — status (default), owner, or project.
  // Anything else (incl. "none") falls back to status, which is the board's
  // native grouping.
  const groupBy = ["status", "owner", "project"].includes(viewParams.group ?? "") ? viewParams.group! : "status";

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

  // Build the column set + bucket each row into a column.
  // - status: the four fixed lanes, mapped via statusToCol.
  // - owner/project: one column per distinct value, derived from the rows
  //   (project null → "(unassigned)").
  const PROJECT_UNASSIGNED = "(unassigned)";
  const columns: Array<{ title: string; accent: string }> = [];
  const grouped: Record<string, FileRow[]> = {};
  const colKey = (f: FileRow): string => {
    if (groupBy === "owner") return f.owner || PROJECT_UNASSIGNED;
    if (groupBy === "project") return f.project || PROJECT_UNASSIGNED;
    return statusToCol[f.status] ?? "Inbox";
  };

  if (groupBy === "status") {
    for (const c of STATUS_COLUMNS) { columns.push(c); grouped[c.title] = []; }
    for (const f of rows) grouped[colKey(f)].push(f);
  } else {
    // Distinct values in row order, so columns stay stable / predictable.
    for (const f of rows) {
      const key = colKey(f);
      if (!(key in grouped)) {
        grouped[key] = [];
        columns.push({ title: key, accent: DYNAMIC_ACCENTS[columns.length % DYNAMIC_ACCENTS.length] });
      }
      grouped[key].push(f);
    }
  }

  const groupLabel = GROUP_FIELDS.find((g) => g.key === groupBy)?.label ?? "Status";
  const colCount = columns.length || 1;

  return (
    <div className="scr">
      <Sidebar nav="files" systems={systems} stats={stats} orgs={orgs} systemActive={filters.system_id ?? systems[0]?.id} />
      <TopBar
        crumbs={sys ? ["Workspace", sys.name, "Board"] : ["Workspace", "Board"]}
      />
      <div className="main">
        <div style={{ padding: "14px 24px 8px", borderBottom: "1px solid var(--border)" }}>
          <div className="board-header" style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12 }}>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span className="t-2xl t-semibold">{title}</span>
                {sys && <Pill tone={sys.tone}><span className="dot" />{sys.name}</Pill>}
                <Pill>{rows.length} file{rows.length === 1 ? "" : "s"}</Pill>
              </div>
              <div className="t-sm t-muted" style={{ marginTop: 2 }}>
                Grouped by <span className="t-semibold" style={{ color: "var(--text)" }}>{groupLabel.toLowerCase()}</span> · {colCount} column{colCount === 1 ? "" : "s"}
              </div>
            </div>
            {canMutate(role) && (
              <div className="board-actions" style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                <Link className="btn primary" href={buildViewHref("/upload", { system_id: filters.system_id, org_id: filters.org_id })}><Ico.upload /> Upload</Link>
              </div>
            )}
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
            <ViewTabs params={viewParams} active="board" />
            <GroupMenu params={viewParams} group={groupBy} base="/files/board" allowNone={false} />
            <ViewFilterBar params={viewParams} base="/files/board" />
          </div>
        </div>

        <div style={{ flex: 1, overflow: "auto", padding: "16px 20px", background: "var(--bg-subtle)" }}>
          {columns.length === 0 ? (
            <div className="t-sm t-subtle" style={{ padding: "8px 4px" }}>No files</div>
          ) : (
          <div className="board-grid" style={{ display: "grid", gridTemplateColumns: `repeat(${colCount}, minmax(260px, 1fr))`, gap: 16, overflowX: "auto" }}>
            {columns.map((c) => (
              <div key={c.title} style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <span style={{ width: 10, height: 10, borderRadius: 3, background: c.accent }} />
                  <span className="t-md t-semibold">{c.title}</span>
                  <span className="t-sm t-muted t-tabular">{grouped[c.title].length}</span>
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
          )}
        </div>
      </div>
    </div>
  );
}
