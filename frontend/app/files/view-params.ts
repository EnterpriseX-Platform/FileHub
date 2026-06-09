// Shared between the server pages (every /files layout) and the client
// toolbar (search / filter / sort).  No "use client" so both sides can import.

import type { FileRow } from "@/lib/api";

export type ViewParams = Record<string, string | undefined>;
export type SearchParamsObj = Record<string, string | string[] | undefined>;

/// Build a URL for any /files layout from the preserved view params, dropping
/// empty values. basePath (`/filehub`) is added automatically by
/// <Link>/useRouter, so we keep the path bare here.
export function buildViewHref(base: string, params: ViewParams): string {
  const sp = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== "") sp.set(k, v);
  }
  const qs = sp.toString();
  return base + (qs ? `?${qs}` : "");
}

/// Same, anchored at the table view.
export function buildFilesHref(params: ViewParams): string {
  return buildViewHref("/files", params);
}

const str = (v: string | string[] | undefined): string | undefined =>
  typeof v === "string" && v.length ? v : undefined;

/// Read the full set of view params from a page's searchParams, so every
/// layout preserves them across the layout tabs (defaults dropped for clean
/// URLs). The filter subset (system_id/org_id/status/project/owner) is also
/// what each page feeds to its data fetch + `fileMatchesFilters`.
export function readViewParams(sp: SearchParamsObj): ViewParams {
  const sort  = str(sp.sort);
  const group = str(sp.group);
  return {
    system_id: str(sp.system_id),
    org_id:    str(sp.org_id),
    status:    str(sp.status),
    project:   str(sp.project),
    owner:     str(sp.owner),
    folder_id: str(sp.folder_id),
    q:         str(sp.q)?.trim() || undefined,
    sort:      sort && sort !== "modified" ? sort : undefined,
    dir:       sp.dir === "asc" ? "asc" : undefined,
    group:     group && group !== "none" ? group : undefined,
    hide:      str(sp.hide),
  };
}

/// The equality-filter subset, as a plain object (only present keys).
export function readFilters(sp: SearchParamsObj): Record<string, string> {
  const f: Record<string, string> = {};
  for (const k of ["system_id", "org_id", "status", "project", "owner"] as const) {
    const v = str(sp[k]);
    if (v) f[k] = v;
  }
  return f;
}

/// Client-side filter so search results (and the backend-ignored `owner`
/// filter) compose with the filter pills across every layout. For the plain
/// list path the backend already filtered, so this is a harmless no-op.
export function fileMatchesFilters(f: FileRow, filters: Record<string, string>): boolean {
  if (filters.system_id && f.system_id !== filters.system_id) return false;
  if (filters.org_id    && f.org_id    !== filters.org_id)    return false;
  if (filters.status    && f.status    !== filters.status)    return false;
  if (filters.project   && f.project   !== filters.project)   return false;
  if (filters.owner     && f.owner     !== filters.owner)     return false;
  return true;
}

// Sort fields the table supports (applied client-side / in the server
// component because the backend hardcodes ORDER BY modified_at DESC).
export const SORT_FIELDS: Array<{ key: string; label: string }> = [
  { key: "modified", label: "Modified" },
  { key: "name",     label: "Name" },
  { key: "size",     label: "Size" },
  { key: "status",   label: "Status" },
  { key: "owner",    label: "Owner" },
];

export const STATUS_OPTIONS = ["Draft", "Review", "Approved", "Archived"];

// Group the table rows by one of these fields (server-side, via `?group=`).
export const GROUP_FIELDS: Array<{ key: string; label: string }> = [
  { key: "none",    label: "No grouping" },
  { key: "status",  label: "Status" },
  { key: "owner",   label: "Owner" },
  { key: "project", label: "Project" },
];

// Optional (hideable) table columns, in render order. "Name" is always shown.
// Hidden columns are carried in `?hide=` as a comma list.
export const OPTIONAL_COLUMNS: Array<{ key: string; label: string }> = [
  { key: "status",   label: "Status" },
  { key: "project",  label: "Project" },
  { key: "owner",    label: "Owner" },
  { key: "size",     label: "Size" },
  { key: "modified", label: "Modified" },
  { key: "tags",     label: "Tags" },
  { key: "versions", label: "Versions" },
];

/// Parse the `?hide=` param into a Set of hidden column keys.
export function parseHidden(hide: string | undefined): Set<string> {
  return new Set((hide ?? "").split(",").map((s) => s.trim()).filter(Boolean));
}
