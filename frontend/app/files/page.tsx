import Link from "next/link";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { UserAvatar } from "@/components/user-avatar";
import { safeFiles, safeFolders, safeOrgs, safeSearch, safeStats, safeSystems } from "@/lib/api";
import type { FileRow, Folder } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { canMutate } from "@/lib/roles";

import { NewFolderButton } from "./new-folder-button";
import { FilesTable, type FileGroup } from "./files-table";
import { FilterMenu, SortMenu, ViewOptionsMenu, ViewSearch } from "./table-toolbar";
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

  // #9 — folder navigation. When ?folder_id= is set we scope the listing to
  // that folder (backend filters server-side); absent = every folder, today's
  // behaviour. Search ignores folder scope (FTS spans the whole workspace).
  const folderId = typeof sp.folder_id === "string" && sp.folder_id.length ? sp.folder_id : undefined;

  const { cookieHeader, role } = await loadServerCtx();
  const [files, systems, stats] = await Promise.all([
    searchTerm
      ? safeSearch(searchTerm, cookieHeader)
      : safeFiles({ ...filters, ...(folderId ? { folder_id: folderId } : {}), limit: String(LIST_LIMIT) }, cookieHeader),
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

  // #9 — resolve the open folder + its ancestor chain (root → current) from the
  // already-fetched folder list, plus the immediate child folders to drill into.
  // Only meaningful inside a single system and when not searching.
  const folderById = new Map(folders.map((f) => [f.id, f]));
  const currentFolder = folderId ? folderById.get(folderId) : undefined;
  const folderTrail: Folder[] = [];
  if (currentFolder) {
    let cur: Folder | undefined = currentFolder;
    const seen = new Set<string>();
    while (cur && !seen.has(cur.id)) { folderTrail.unshift(cur); seen.add(cur.id); cur = cur.parent_id ? folderById.get(cur.parent_id) : undefined; }
  }
  // Child folders shown as drill-in rows: direct children of the open folder,
  // or the system's root folders when no folder is open. Hidden while searching.
  const showFolderNav = !searchTerm && !!activeSysId;
  const childFolders = showFolderNav
    ? folders.filter((f) => (folderId ? f.parent_id === folderId : f.parent_id === null))
    : [];

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
      case "project": return f.project || "Unassigned";
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
    folder_id: folderId,
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
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12, flexWrap: "wrap" }}>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span className="t-2xl t-semibold">{title}</span>
                {sys && <Pill tone={sys.tone}><span className="dot" />{sys.name}</Pill>}
                <Pill>{rows.length} file{rows.length === 1 ? "" : "s"}</Pill>
              </div>
              <div className="t-sm t-muted" style={{ marginTop: 2 }}>
                {sys
                  ? <>{folders.length} folder{folders.length === 1 ? "" : "s"} in this system</>
                  : <>Workspace-wide view across {systems.length} system{systems.length === 1 ? "" : "s"}</>
                }
              </div>
            </div>
            {canMutate(role) && (
              <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                <NewFolderButton systemId={activeSysId} systemName={(sys ?? systems.find((s) => s.id === activeSysId))?.name} orgId={filters.org_id} parentId={folderId} />
                {/* Hand the current location to /upload so files land where
                    the user is standing (system/org/folder). */}
                <Link className="btn primary" href={buildViewHref("/upload", { system_id: filters.system_id, org_id: filters.org_id, folder_id: folderId })}><Ico.upload /> Upload</Link>
              </div>
            )}
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
            <ViewTabs params={viewParams} active="table" />
            <div className="divider-v" style={{ height: 18 }} />
            <div style={{ display: "flex", gap: 6 }}>
              <FilterMenu params={viewParams} count={activeFilters.length} />
              <SortMenu params={viewParams} sort={sortKey} dir={sortDir} />
            </div>
            <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
              <ViewSearch params={viewParams} />
              {/* Grouping, column visibility, and save-as-view are occasional
                  controls — one menu instead of three toolbar buttons. */}
              <ViewOptionsMenu params={viewParams} group={groupBy} hidden={[...hidden]} />
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

        {showFolderNav && (folderTrail.length > 0 || childFolders.length > 0) && (
          <div style={{ padding: "10px 24px", borderBottom: "1px solid var(--border)", display: "flex", flexDirection: "column", gap: 10 }}>
            {/* Breadcrumb: system root → ancestor folders → current */}
            <div style={{ display: "flex", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
              <Link
                href={buildFilesHref({ ...viewParams, folder_id: undefined })}
                className="btn xs ghost"
                style={{ gap: 4 }}
                title={`${(sys ?? systems.find((sy) => sy.id === activeSysId))?.name ?? "System"} root`}
              >
                <Ico.home className="icon sm" />
                {(sys ?? systems.find((sy) => sy.id === activeSysId))?.name ?? "Files"}
              </Link>
              {folderTrail.map((f, i) => {
                const last = i === folderTrail.length - 1;
                return (
                  <span key={f.id} style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                    <Ico.chevron className="icon sm" style={{ color: "var(--text-subtle)" }} />
                    {last ? (
                      <span className="t-sm t-semibold" style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                        <Ico.folderOpen className="icon sm" style={{ color: f.color ?? "var(--text-muted)" }} />
                        {f.name}
                      </span>
                    ) : (
                      <Link href={buildFilesHref({ ...viewParams, folder_id: f.id })} className="btn xs ghost" style={{ gap: 4 }}>
                        <Ico.folder className="icon sm" />
                        {f.name}
                      </Link>
                    )}
                  </span>
                );
              })}
            </div>

            {/* Child folders to drill into */}
            {childFolders.length > 0 && (
              <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
                {folderId && (
                  <Link
                    href={buildFilesHref({ ...viewParams, folder_id: currentFolder?.parent_id ?? undefined })}
                    className="btn sm ghost"
                    style={{ gap: 6 }}
                    title="Up one level"
                  >
                    <Ico.up className="icon sm" /> Up
                  </Link>
                )}
                {childFolders.map((f) => (
                  <Link
                    key={f.id}
                    href={buildFilesHref({ ...viewParams, folder_id: f.id })}
                    className="btn sm"
                    style={{ gap: 8 }}
                    title={`Open ${f.name}`}
                  >
                    <Ico.folder className="icon sm" style={{ color: f.color ?? "var(--text-muted)" }} />
                    {f.name}
                    <Ico.chevron className="icon sm" style={{ color: "var(--text-subtle)" }} />
                  </Link>
                ))}
              </div>
            )}
          </div>
        )}

        <div style={{ flex: 1, overflow: "auto", padding: "0 24px" }}>
          {rows.length === 0 ? (
            <div style={{ padding: 40, textAlign: "center", color: "var(--text-subtle)" }}>
              {searchTerm || activeFilters.length > 0
                ? <>No files match this view. <Link href={buildFilesHref({})} className="t-semibold" style={{ color: "var(--accent)" }}>Clear filters</Link>.</>
                : <>No files here yet —{canMutate(role) && <> <Link href={buildViewHref("/upload", { system_id: filters.system_id, org_id: filters.org_id, folder_id: folderId })} className="t-semibold" style={{ color: "var(--accent)" }}>Upload your first file</Link>.</>}</>}
            </div>
          ) : (
            <div className="table-scroll">
              <FilesTable groups={groups} cols={visibleCols} role={role} folders={folders} />
            </div>
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
