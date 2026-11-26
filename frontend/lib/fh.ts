"use client";

import * as React from "react";

/// Client-side API for the basket screens. Every call goes through the
/// Next.js basePath proxy (/filehub/api/* → backend /fh/api/*), so the
/// session cookie travels with it. Errors carry the backend's own message.

export const BASE_PATH = "/filehub";

export class ApiErr extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

/// The sentence to show for a failed request: the server's own (it always
/// sends one, with a machine-readable `code`), else one based on the status.
export function messageFor(status: number, body: unknown): string {
  const server = body && typeof body === "object" && "error" in body ? String((body as { error: unknown }).error) : "";
  // A 403 page that is not ours (HTML from the edge WAF) is not a permission problem.
  if (!server && status === 403 && typeof body === "string" && /<html|cloudflare/i.test(body))
    return "The network security system blocked this request. Contact your administrator.";
  return server || (status === 401 ? "Your session has expired. Sign in again."
    : status === 403 ? "You don't have permission to do this."
    : status === 404 ? "This item doesn't exist or you don't have access to it."
    : status === 413 ? "The file is too large."
    : `Request failed (${status})`);
}

export async function api<T = unknown>(path: string, init: RequestInit & { json?: unknown } = {}): Promise<T> {
  const { json, ...rest } = init;
  const headers = new Headers(rest.headers);
  let body = rest.body;
  if (json !== undefined) {
    headers.set("content-type", "application/json");
    body = JSON.stringify(json);
  }
  // The edge in front of FileHub lets only GET and POST through: other methods go
  // as POST carrying the real one in X-HTTP-Method-Override (the server switches it back).
  let method = (rest.method ?? "GET").toUpperCase();
  if (method === "PUT" || method === "PATCH" || method === "DELETE") {
    headers.set("X-HTTP-Method-Override", method);
    method = "POST";
  }
  const res = await fetch(`${BASE_PATH}/api${path}`, { cache: "no-store", credentials: "same-origin", ...rest, method, headers, body });
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  let data: unknown = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = text; }
  if (!res.ok) {
    const msg = messageFor(res.status, data);
    const code = data && typeof data === "object" && "code" in data ? String((data as { code: unknown }).code) : "";
    if (res.status === 401 && code === "unauthorized" && typeof window !== "undefined") {
      const next = encodeURIComponent(window.location.pathname.replace(BASE_PATH, "") + window.location.search);
      window.location.href = `${BASE_PATH}/login?next=${next}`;
    }
    throw new ApiErr(res.status, msg);
  }
  return data as T;
}

export type Basket = {
  id: string; name: string; bucket: string; status: string; description: string | null; tone?: string;
  system_type: "shared" | "personal"; owner_user_id: string | null; quota_bytes: number; vault?: boolean;
  mount_path?: string | null; mount_scanned_at?: string | null;
};
export type Folder = {
  id: string; system_id: string; parent_id: string | null; name: string; color: string | null;
  owner: string; created_at: string; inherit: boolean;
};
export type FileRow = {
  id: string; name: string; file_type: string; size_bytes: number; system_id: string; org_id: string | null;
  status: string; owner: string; tags: string; version: number; metadata: string; created_at: string;
  modified_at: string; folder_id: string | null; encrypted: boolean; created_by: string | null; category_code: string | null;
};
export type Category = { id?: string; parent_code: string | null; code: string; name: string; sort?: number };
export type FieldDef = {
  key: string; label: string; field_type: "text" | "number" | "date" | "choice" | "user" | "org";
  required: boolean; choices: (string | { value: string; label: string })[]; searchable: boolean; show_in_list: boolean; sort?: number;
};
export type StatusDef = { code: string; label: string; color: TagColor; is_initial: boolean; locks_content: boolean; awaits_review?: boolean; sort?: number };
export type Transition = { from_code: string; to_code: string; required_level: number };
export type Policy = { encrypt: boolean | null; allowed_ext: string[] | null; max_file_bytes: number | null; public_links: boolean | null; stamp_downloads?: boolean | null;
  encrypt_mode?: string | null; file_encrypt_choice?: boolean | null; file_encrypt_mode?: string | null; content_search?: boolean | null };
export type Settings = { categories: Category[]; fields: FieldDef[]; statuses: StatusDef[]; transitions: Transition[]; policy: Policy; use_statuses?: boolean };
export type TagColor = "gray" | "blue" | "green" | "orange" | "red";

export type AclEntry = {
  id: string; resource_type: string; resource_id: string; principal: string; level: number; level_name: string;
  principal_label: string; inherited_from: string | null; expires_at: string | null; granted_by_label?: string | null;
};
export type AclResponse = { resource_type: string; resource_id: string; inherit: boolean; entries: AclEntry[]; inherited: AclEntry[]; my_level: string };
export type Principal = { principal: string; label: string; kind: string; detail: string | null };
export type Effective = { level: "view" | "edit" | "manage" | "none"; actions: string[] };

export type Rule =
  | { field: string; op: string; value?: unknown }
  | { match: "all" | "any"; rules: Rule[] };
export type VBasket = {
  id: string; name: string; description: string | null; match_mode: "all" | "any"; rules: Rule[]; group_by: string[];
  all_sources: boolean; drop_target_system_id: string | null; drop_target_folder_id: string | null; columns: string[];
  personal: boolean; shared_with: string[]; created_by: string; sources: string[]; source_names: string[]; can_edit: boolean;
  drop_target_label: string | null; group_by_labels: string[]; prefill: { category_code?: string; status?: string; org_id?: string; fields?: Record<string, string> };
};
export type VItem = {
  id: string; name: string; system_id: string; basket_name: string; size_bytes: number; status: string;
  category_code: string | null; modified_at: string; folder_id: string | null; metadata: string; created_by: string | null;
};
export type VTree = {
  level: number; level_field: string | null; level_label: string | null;
  path: { value: string | null; label: string }[];
  groups: { value: string | null; label: string; count: number; bytes: number; modified_at: string | null }[];
  items: VItem[]; total: number; bytes: number;
};
export type FieldOption = { key: string; label: string; kind: string; choices: { value: string; label: string }[]; system: boolean };
export type DlItem = { kind: "file"; id: string } | { kind: "folder"; id: string } | { kind: "vgroup"; vbasket_id: string; path: string[] };
export type DlEntry = { item: DlItem; label: string; files: number };

// ── Formatting ────────────────────────────────────────────────────────────
const GB = 1024 ** 3;
/// Bytes → whole GB as a form value ("" for none).
export const toGbField = (n: number | null | undefined) => (n ? String(Math.round(n / GB)) : "");
/// A GB form value → bytes (0 = no limit).
export const fromGbField = (v: string) => (v.trim() ? Math.round(Number(v) * GB) : 0);

export function bytes(n: number | null | undefined): string {
  if (n == null) return "";
  if (n >= 1073741824) return `${(n / 1073741824).toFixed(1)} GB`;
  if (n >= 1048576) return `${(n / 1048576).toFixed(1)} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
export function when(iso: string | null | undefined, time = true): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const base = `${d.getDate()} ${MONTHS[d.getMonth()]} ${d.getFullYear()}`;
  return time ? `${base} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}` : base;
}
export function ext(name: string): string {
  const m = /\.([^./]+)$/.exec(name);
  return m ? m[1].toUpperCase().slice(0, 4) : "FILE";
}
export function meta(m: string | null | undefined): Record<string, unknown> {
  if (!m) return {};
  try { const v = JSON.parse(m); return v && typeof v === "object" && !Array.isArray(v) ? v : {}; } catch { return {}; }
}
export function choiceList(f: FieldDef): { value: string; label: string }[] {
  return (f.choices || []).map((c) => (typeof c === "string" ? { value: c, label: c } : { value: String(c.value), label: String(c.label ?? c.value) }));
}
export function href(path: string): string {
  return `${BASE_PATH}${path}`;
}

/// Load `path` once (and again on `reload()`): `{ data, err, reload }`.
/// `path = null` waits (nothing is fetched until a path is given).
export function useApi<T>(path: string | null) {
  const [data, setData] = React.useState<T | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const reload = React.useCallback(() => { if (path == null) return; api<T>(path).then((d) => { setData(d); setErr(null); }).catch((e) => setErr((e as Error).message)); }, [path]);
  React.useEffect(() => { reload(); }, [reload]);
  return { data, err, reload };
}
