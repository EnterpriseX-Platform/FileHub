import Link from "next/link";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { UserAvatar } from "@/components/user-avatar";
import { safeFiles, safeFolders, safeOrgs, safeSearch, safeStats, safeSystems } from "@/lib/api";
import type { FileRow } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { canMutate } from "@/lib/roles";

import { NewFolderButton } from "./new-folder-button";
import { FilesTable, type FileGroup } from "./files-table";
import { FilterMenu, GroupMenu, PropertiesMenu, SortMenu, ViewSearch } from "./table-toolbar";
import { ViewTabs } from "./view-tabs";
import { buildFilesHref, buildViewHref, fileMatchesFilters, OPTIONAL_COLUMNS, parseHidden, type ViewParams } from "./view-params";

type FilesPageProps = { searchParams?: Promise<Record<string, string | string[] | undefined>> };

const PAGE_SIZE = 50;
// We fetch up to this many rows, then filter/sort/paginate client-side (the
// backend can't sort or compose filters). Beyond this the footer flags the cap.
const LIST_LIMIT = 1000;

export default async function FilesTablePage({ searchParams }: FilesPageProps) {
  const sp = (await searchParams) ?? {};
  const filters: Record<string, string> = {};
  for (const k of ["system_id", "org_id", "status", "project", "owner"] as const) {
    const v = sp[k];
    if (typeof v === "string" && v.length) filters[k] = v;
  }
  // Q7 — when ?q= is present we hit /api/search (FTS over name/tags/owner +
  // extracted PDF/txt content) instead of /api/files.  Treat the rest of
  // the filter pills as informational since the search endpoint applies its
  // own subset.
  const searchTerm = typeof sp.q === "string" ? sp.q.trim() : "";

  const { cookieHeader, role } = await loadServerCtx();
  const [files, systems, stats] = await Promise.all([
    searchTerm ? safeSearch(searchTerm, cookieHeader) : safeFiles({ ...filters, limit: String(LIST_LIMIT) }, cookieHeader),
    safeSystems(cookieHeader),
    safeStats(cookieHeader),
  ]);
  const activeSysId = filters.system_id ?? systems[0]?.id;
  const [orgs, folders] = await Promise.all([
    activeSysId ? safeOrgs(activeSysId, cookieHeader) : Promise.resolve([]),
    activeSysId ? safeFolders(activeSysId, cookieHeader) : Promise.resolve([]),
  ]);

  // Choose a sensible title + breadcrumb from the query string instead of
  // mocking a fixed location. When no filter is active we fall back to
  // a global "All files" view.
  const sys = filters.system_id ? systems.find((s) => s.id === filters.system_id) : undefined;
  const title = searchTerm ? `Search: "${searchTerm}"` :
                filters.status ? `${filters.status} files` :
                filters.project ? `${filters.project} files` :
                sys ? `${sys.name} files` :
                "All files";
  const crumbs = searchTerm ? ["Workspace", "Search"] :
                 sys ? ["Workspace", sys.name, "Files"] :
                 ["Workspace", "Files"];
  const activeFilters = Object.entries(filters);

  // Sort is applied here in the server component because the backend hardcodes
  // `ORDER BY modified_at DESC`. Default = modified desc (same as the backend).
  // When searching we keep the search-relevance order unless the user picks a
  // sort explicitly.
  const explicitSort = typeof sp.sort === "string";
  const sortKey = explicitSort ? (sp.sort as string) : "modified";
  const sortDir = sp.dir === "asc" ? "asc" : "desc";

  // Apply filters client-side so they compose with search (the FTS endpoint
  // doesn't take filter params) and so the backend-ignored `owner` filter works.
  // For the plain-list path the backend already filtered, so this is a no-op.
  let rows = files.filter((f) => fileMatchesFilters(f, filters));
  if (explicitSort || !searchTerm) {
    rows = [...rows].sort((a, b) => {
      let c = 0;
      switch (sortKey) {
        case "name":   c = a.name.localeCompare(b.name); break;
        case "size":   c = a.size_bytes - b.size_bytes; break;
        case "status": c = a.status.localeCompare(b.status); break;
        case "owner":  c = a.owner.localeCompare(b.owner); break;
        default:       c = new Date(a.modified_at).getTime() - new Date(b.modified_at).getTime();
      }
      return sortDir === "asc" ? c : -c;
    });
  }

  // Grouping + column visibility, decided server-side and carried in
  // ?group / ?hide. The client toolbar menus just set these params.
  const groupBy = typeof sp.group === "string" ? sp.group : "none";
  const hidden = parseHidden(typeof sp.hide === "string" ? sp.hide : undefined);
  const visibleCols = OPTIONAL_COLUMNS.map((c) => c.key).filter((k) => !hidden.has(k));

  // Paginate the filtered+sorted rows (client-side slice, since sort/filters are
  // applied here). Skipped while grouping — groups want the whole overview.
  const total = rows.length;
  const paginate = groupBy === "none";
  const pageCount = paginate ? Math.max(1, Math.ceil(total / PAGE_SIZE)) : 1;
  const page = paginate
    ? Math.min(Math.max(0, parseInt(typeof sp.page === "string" ? sp.page : "0", 10) || 0), pageCount - 1)
    : 0;
  const pageRows = paginate ? rows.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE) : rows;

  const groupValue = (f: FileRow): string => {
    switch (groupBy) {
      case "status":  return f.status || "—";
      case "owner":   return f.owner || "—";
      case "project": return f.project || "(no project)";
      default:        return "";
    }
  };
  let groups: FileGroup[];
  if (groupBy === "none") {
    groups = [{ key: "all", label: "", rows: pageRows }];
  } else {
    const map = new Map<string, FileRow[]>();
    for (const f of pageRows) {
      const v = groupValue(f);
      let bucket = map.get(v);
      if (!bucket) { bucket = []; map.set(v, bucket); }
      bucket.push(f);
    }
    groups = [...map.entries()].map(([label, rs]) => ({ key: label, label, rows: rs }));
  }

  // Params preserved as the user searches / filters / sorts / groups within the
  // view, shared with the client toolbar and the removable filter pills.
  // Defaults (modified/desc/no-group/all-columns) are dropped so URLs stay clean.
  const viewParams: ViewParams = {
    system_id: filters.system_id,
    org_id:    filters.org_id,
    status:    filters.status,
    project:   filters.project,
    owner:     filters.owner,
    q:         searchTerm || undefined,
    sort:      sortKey !== "modified" ? sortKey : undefined,
    dir:       sortDir !== "desc" ? sortDir : undefined,
    group:     groupBy !== "none" ? groupBy : undefined,
    hide:      hidden.size ? OPTIONAL_COLUMNS.filter((c) => hidden.has(c.key)).map((c) => c.key).join(",") : undefined,
  };

  return (
    <div className="scr">
      <Sidebar nav="files" systems={systems} stats={stats} orgs={orgs} systemActive={activeSysId} orgActive={filters.org_id} />
      <TopBar
        crumbs={crumbs}
        actions={
          <>
            <UserAvatar />
          </>
        }
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
                {sys
                  ? <>Stored in bucket <span className="t-mono">{sys.bucket}</span> · {folders.length} folder{folders.length === 1 ? "" : "s"}</>
                  : <>Workspace-wide view across {systems.length} system{systems.length === 1 ? "" : "s"}</>
                }
              </div>
            </div>
            {canMutate(role) && (
              <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                <NewFolderButton systemId={activeSysId} systemName={(sys ?? systems.find((s) => s.id === activeSysId))?.name} orgId={filters.org_id} />
                <a className="btn primary" href="/upload"><Ico.upload /> Upload</a>
              </div>
            )}
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <ViewTabs params={viewParams} active="table" />
            <div className="divider-v" style={{ height: 18 }} />
            <div style={{ display: "flex", gap: 6 }}>
              <FilterMenu params={viewParams} count={activeFilters.length} />
              <SortMenu params={viewParams} sort={sortKey} dir={sortDir} />
              <GroupMenu params={viewParams} group={groupBy} />
              <PropertiesMenu params={viewParams} hidden={[...hidden]} />
            </div>
            <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
              <ViewSearch params={viewParams} />
              <a className="btn sm" href={buildViewHref("/views/new", viewParams)}>Save as new view</a>
            </div>
          </div>

          {activeFilters.length > 0 && (
            <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 10 }}>
              <span className="t-xs t-subtle">FILTERS:</span>
              {activeFilters.map(([k, v]) => (
                <Pill key={k} tone="indigo">
                  <span style={{ fontWeight: 600 }}>{k}</span> = {`"${v}"`}
                  <Link
                    href={buildFilesHref({ ...viewParams, [k]: undefined })}
                    title={`Remove ${k} filter`}
                    style={{ display: "inline-flex", alignItems: "center", marginLeft: 4, color: "inherit", opacity: 0.65 }}
                  >
                    <Ico.x className="icon sm" />
                  </Link>
                </Pill>
              ))}
            </div>
          )}
        </div>

        <div style={{ flex: 1, overflow: "auto", padding: "0 24px" }}>
          {rows.length === 0 ? (
            <div style={{ padding: 40, textAlign: "center", color: "var(--text-subtle)" }}>
              {searchTerm || activeFilters.length > 0
                ? <>No files match this view. <Link href={buildFilesHref({})} className="t-semibold" style={{ color: "var(--accent)" }}>Clear filters</Link>.</>
                : <>No files yet.{canMutate(role) && <> <a href="/upload" className="t-semibold" style={{ color: "var(--accent)" }}>Upload your first file</a>.</>}</>}
            </div>
          ) : (
            <FilesTable groups={groups} cols={visibleCols} role={role} />
          )}
        </div>

        <div style={{ borderTop: "1px solid var(--border)", padding: "8px 24px", display: "flex", alignItems: "center", gap: 12, background: "var(--bg-subtle)" }}>
          <div className="t-sm t-muted">
            {total === 0
              ? "No files"
              : paginate
                ? `Showing ${page * PAGE_SIZE + 1}–${Math.min((page + 1) * PAGE_SIZE, total)} of ${total}`
                : `${total} file${total === 1 ? "" : "s"}`}
            {total >= LIST_LIMIT && <span className="t-subtle"> · capped at {LIST_LIMIT}, narrow with filters</span>}
          </div>
          {paginate && pageCount > 1 && (
            <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
              {page > 0
                ? <Link className="btn xs ghost" href={buildFilesHref({ ...viewParams, page: page - 1 === 0 ? undefined : String(page - 1) })}>‹ Prev</Link>
                : <span className="btn xs ghost" style={{ opacity: 0.4, pointerEvents: "none" }}>‹ Prev</span>}
              <span className="t-xs t-subtle t-tabular">Page {page + 1} / {pageCount}</span>
              {page < pageCount - 1
                ? <Link className="btn xs ghost" href={buildFilesHref({ ...viewParams, page: String(page + 1) })}>Next ›</Link>
                : <span className="btn xs ghost" style={{ opacity: 0.4, pointerEvents: "none" }}>Next ›</span>}
            </div>
          )}
          <div style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
            <span className="t-xs t-subtle">Local storage · live</span>
            <span style={{ width: 6, height: 6, borderRadius: 3, background: "var(--success)" }} />
          </div>
        </div>
      </div>
    </div>
  );
}
