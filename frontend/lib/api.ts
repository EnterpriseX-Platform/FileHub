import type { Tone } from "@/components/primitives";

// Server-side fetches hit the backend directly under /fh; client-side fetches
// flow through the Next.js basePath/rewrite at /filehub/api/* (which proxies to
// the same /fh/api/* path on the backend).  See next.config.ts.
// Force IPv4 — `localhost` resolves to `::1` on macOS Sonoma+, but the Rust
// backend binds `0.0.0.0` by default.  Without `127.0.0.1` every server-side
// fetch from a Next.js page would hit `ECONNREFUSED ::1:8090`.
const BASE = (process.env.BACKEND_URL || "http://127.0.0.1:8090") + "/fh";

async function get<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, { cache: "no-store", ...init });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText} at ${path}`);
  return res.json() as Promise<T>;
}

export type System = {
  id: string;
  name: string;
  tone: Tone;
  bucket: string;
  status: string;
  description: string | null;
  created_at: string;
  /// 'shared' (workspace-wide) or 'personal' (per-user My Drive).
  system_type: "shared" | "personal";
  /// Owner for personal drives; null for shared systems.
  owner_user_id: string | null;
  /// 0 = no system-level cap (inherit workspace).
  quota_bytes: number;
  deleted_at: string | null;
  count?: string | number;
};

export type Org = {
  id: string;
  system_id: string;
  name: string;
  code: string;
  tier: string;
  owner: string;
  tags: string;
  status: string;
  created_at: string;
  quota_bytes: number;
};

// Phase J — /api/quota
export type QuotaScope = {
  scope: "workspace" | "system" | "org" | "user";
  id: string | null;
  limit_bytes: number;
  used_bytes: number;
};
export type QuotaReport = { scopes: QuotaScope[] };

// Q1/Q5 — workspace_config k/v (workspace name + access policy booleans)
export type WorkspaceConfig = Record<string, string>;

// Q2 — members CRUD
export type Member = {
  id: string;
  email: string;
  display_name: string;
  avatar_tone: string;
  role: "admin" | "editor" | "viewer";
  status: "active" | "disabled";
  quota_bytes: number;
  used_bytes: number;
  created_at: string;
};

// Q6 — notifications
export type Notification = {
  id: string;
  user_id: string;
  kind: string;
  title: string;
  body: string | null;
  link: string | null;
  read_at: string | null;
  created_at: string;
};

// Phase K — rotation
export type RotationPolicy = {
  id: string;
  scope_type: "workspace" | "system" | "org" | "user";
  scope_id: string | null;
  keep_last_n_versions: number;
  archive_after_days: number;
  delete_after_days: number;
  created_at: string;
  updated_at: string;
};
export type RotationRun = {
  id: string;
  started_at: string;
  finished_at: string | null;
  versions_pruned: number;
  files_archived: number;
  files_hard_deleted: number;
  triggered_by: string | null;
  error: string | null;
};

export type FileRow = {
  id: string;
  name: string;
  file_type: string;
  size_bytes: number;
  system_id: string;
  org_id: string | null;
  bucket: string;
  object_key: string;
  project: string | null;
  status: string;
  owner: string;
  tags: string;
  version: number;
  metadata: string;
  etag: string | null;
  created_at: string;
  modified_at: string;
  folder_id: string | null;
  encrypted: boolean;
};

export type Activity = {
  id: string;
  actor: string;
  actor_tone: Tone;
  action: string;
  target: string | null;
  target_type: string | null;
  file_id: string | null;
  system_id: string | null;
  org_id: string | null;
  created_at: string;
};

export type SystemStorage = {
  system_id: string;
  name: string;
  tone: Tone;
  size_bytes: number;
  file_count: number;
};

export type ConnectedSystem = {
  id: string;
  name: string;
  tone: Tone;
  status: string;
  file_count: number;
};

export type DashboardStats = {
  total_files: number;
  total_size_bytes: number;
  total_quota_bytes: number;
  active_orgs: number;
  total_orgs: number;
  awaiting_review: number;
  storage_by_system: SystemStorage[];
  connected_systems: ConnectedSystem[];
  workspace_name: string;
  workspace_display: string;
  /// Backend label from the server, e.g. "filesystem(/abs/path)" or
  /// "s3(bucket@endpoint)". Surfaced on the settings page.
  storage_backend: string;
  encryption_enabled: boolean;
};

export type View = {
  id: string;
  name: string;
  layout: string;
  source: string;
  filters: string;
  group_by: string | null;
  sort_by: string | null;
  fields: string;
  pinned: boolean;
  color: string | null;
  created_at: string;
};

export type Permission = {
  id: string;
  file_id: string | null;
  org_id: string | null;
  principal: string;
  principal_type: string;
  role: string;
  external: boolean;
  created_at: string;
};

export const api = {
  stats:      () => get<DashboardStats>("/api/stats"),
  systems:    () => get<System[]>("/api/systems"),
  orgs:       (system_id?: string) => get<Org[]>(`/api/orgs${system_id ? `?system_id=${encodeURIComponent(system_id)}` : ""}`),
  files:      (q: Record<string, string> = {}) => {
    const params = new URLSearchParams(q).toString();
    return get<FileRow[]>(`/api/files${params ? `?${params}` : ""}`);
  },
  file:       (id: string) => get<FileRow>(`/api/files/${encodeURIComponent(id)}`),
  activity:   (limit = 20) => get<Activity[]>(`/api/activity?limit=${limit}`),
  views:      () => get<View[]>("/api/views"),
  permissions:(file_id: string) => get<Permission[]>(`/api/permissions/${encodeURIComponent(file_id)}`),
};

export async function safeViews(cookieHeader?: string): Promise<View[]> {
  if (cookieHeader === undefined) {
    try { return await api.views(); } catch { return []; }
  }
  try {
    const r = await fetch(`${BASE}/api/views`, {
      cache: "no-store",
      headers: { cookie: cookieHeader },
    });
    if (!r.ok) return [];
    return (await r.json()) as View[];
  } catch { return []; }
}

export type Folder = {
  id: string;
  system_id: string;
  org_id: string | null;
  parent_id: string | null;
  name: string;
  color: string | null;
  owner: string;
  encrypted: boolean;
  created_at: string;
};

export async function safeFolders(system_id?: string, cookieHeader?: string): Promise<Folder[]> {
  try {
    const qs = system_id ? `?system_id=${encodeURIComponent(system_id)}` : "";
    const res = await fetch(`${BASE}/api/folders${qs}`, {
      cache: "no-store",
      headers: cookieHeader ? { cookie: cookieHeader } : undefined,
    });
    if (!res.ok) return [];
    return (await res.json()) as Folder[];
  } catch {
    return [];
  }
}

export async function safeStats(cookieHeader?: string): Promise<DashboardStats | null> {
  if (cookieHeader === undefined) {
    try { return await api.stats(); } catch { return null; }
  }
  try {
    const r = await fetch(`${BASE}/api/stats`, {
      cache: "no-store",
      headers: { cookie: cookieHeader },
    });
    if (!r.ok) return null;
    return (await r.json()) as DashboardStats;
  } catch { return null; }
}

export async function safeFiles(q: Record<string, string> = {}, cookieHeader?: string): Promise<FileRow[]> {
  if (cookieHeader === undefined) {
    try { return await api.files(q); } catch { return []; }
  }
  try {
    const params = new URLSearchParams(q).toString();
    const r = await fetch(`${BASE}/api/files${params ? `?${params}` : ""}`, {
      cache: "no-store",
      headers: { cookie: cookieHeader },
    });
    if (!r.ok) return [];
    return (await r.json()) as FileRow[];
  } catch { return []; }
}

export async function safeOrgs(system_id?: string, cookieHeader?: string): Promise<Org[]> {
  if (cookieHeader === undefined) {
    try { return await api.orgs(system_id); } catch { return []; }
  }
  try {
    const qs = system_id ? `?system_id=${encodeURIComponent(system_id)}` : "";
    const r = await fetch(`${BASE}/api/orgs${qs}`, {
      cache: "no-store",
      headers: { cookie: cookieHeader },
    });
    if (!r.ok) return [];
    return (await r.json()) as Org[];
  } catch { return []; }
}

export async function safeSystems(cookieHeader?: string): Promise<System[]> {
  // When called from a server component, pass the inbound cookie so the
  // private /api/systems endpoint sees a session.  Without it the backend
  // returns 401 and the sidebar comes up empty (no systems pills).
  if (cookieHeader === undefined) {
    try { return await api.systems(); } catch { return []; }
  }
  try {
    const r = await fetch(`${BASE}/api/systems`, {
      cache: "no-store",
      headers: { cookie: cookieHeader },
    });
    if (!r.ok) return [];
    return (await r.json()) as System[];
  } catch { return []; }
}

export async function safeActivity(limit = 20, cookieHeader?: string): Promise<Activity[]> {
  // Same cookie-forwarding story as safeSystems: /api/activity is private,
  // so server components rendering it (e.g. /settings/audit) must forward
  // the inbound cookie or every row disappears.
  if (cookieHeader === undefined) {
    try { return await api.activity(limit); } catch { return []; }
  }
  try {
    const r = await fetch(`${BASE}/api/activity?limit=${limit}`, {
      cache: "no-store",
      headers: { cookie: cookieHeader },
    });
    if (!r.ok) return [];
    return (await r.json()) as Activity[];
  } catch { return []; }
}

export async function safePermissions(file_id: string, cookieHeader?: string): Promise<Permission[]> {
  if (cookieHeader === undefined) {
    try { return await api.permissions(file_id); } catch { return []; }
  }
  try {
    const r = await fetch(`${BASE}/api/permissions/${encodeURIComponent(file_id)}`, {
      cache: "no-store",
      headers: { cookie: cookieHeader },
    });
    if (!r.ok) return [];
    return (await r.json()) as Permission[];
  } catch { return []; }
}

export async function safeFile(id: string, cookieHeader?: string): Promise<FileRow | null> {
  if (cookieHeader === undefined) {
    try { return await api.file(id); } catch { return null; }
  }
  try {
    const r = await fetch(`${BASE}/api/files/${encodeURIComponent(id)}`, {
      cache: "no-store",
      headers: { cookie: cookieHeader },
    });
    if (!r.ok) return null;
    return (await r.json()) as FileRow;
  } catch { return null; }
}

// Phase H/I/J/K helpers — admin pages call these from server components, so
// they read the backend directly (cookie forwarded by the caller).
export async function safeSystemsWithPersonal(cookieHeader?: string): Promise<System[]> {
  try {
    const r = await fetch(`${BASE}/api/systems?include_personal=true`, {
      cache: "no-store",
      headers: cookieHeader ? { cookie: cookieHeader } : undefined,
    });
    if (!r.ok) return [];
    return (await r.json()) as System[];
  } catch { return []; }
}

export async function safeRotationPolicies(cookieHeader?: string): Promise<RotationPolicy[]> {
  try {
    const r = await fetch(`${BASE}/api/rotation/policies`, {
      cache: "no-store",
      headers: cookieHeader ? { cookie: cookieHeader } : undefined,
    });
    if (!r.ok) return [];
    return (await r.json()) as RotationPolicy[];
  } catch { return []; }
}

export async function safeRotationRuns(cookieHeader?: string): Promise<RotationRun[]> {
  try {
    const r = await fetch(`${BASE}/api/rotation/runs`, {
      cache: "no-store",
      headers: cookieHeader ? { cookie: cookieHeader } : undefined,
    });
    if (!r.ok) return [];
    return (await r.json()) as RotationRun[];
  } catch { return []; }
}

export async function safeWorkspaceConfig(cookieHeader?: string): Promise<WorkspaceConfig> {
  try {
    const r = await fetch(`${BASE}/api/workspace`, {
      cache: "no-store",
      headers: cookieHeader ? { cookie: cookieHeader } : undefined,
    });
    if (!r.ok) return {};
    return (await r.json()) as WorkspaceConfig;
  } catch { return {}; }
}

export async function safeMembers(cookieHeader?: string): Promise<Member[]> {
  try {
    const r = await fetch(`${BASE}/api/users`, {
      cache: "no-store",
      headers: cookieHeader ? { cookie: cookieHeader } : undefined,
    });
    if (!r.ok) return [];
    return (await r.json()) as Member[];
  } catch { return []; }
}

/// Server-side wrapper around `/api/search` for the new file-search results
/// page.  Returns an empty array on backend errors so the UI degrades to "no
/// results" rather than crashing.
export async function safeSearch(q: string, cookieHeader?: string): Promise<FileRow[]> {
  if (!q.trim()) return [];
  try {
    const r = await fetch(`${BASE}/api/search?q=${encodeURIComponent(q)}`, {
      cache: "no-store",
      headers: cookieHeader ? { cookie: cookieHeader } : undefined,
    });
    if (!r.ok) return [];
    return (await r.json()) as FileRow[];
  } catch { return []; }
}
