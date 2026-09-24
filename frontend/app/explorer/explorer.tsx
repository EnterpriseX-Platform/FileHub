"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill, type Tone } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import type { DashboardStats, FileRow, Folder, System, View } from "@/lib/api";
import { fmtAgo, fmtBytes, fmtCount } from "@/lib/format";
import { mutate } from "@/lib/mutate";

import { BucketForm } from "../settings/systems-panel";

// ─────────────────────────────────────────────────────────────────────────────
// Types + data helpers
// ─────────────────────────────────────────────────────────────────────────────

type TagRow = { tag: string; key: string; value: string; count: number };
type Me = { id: string; role: string; display_name: string };
type GroupBy = "none" | "type" | "status" | "owner" | "month" | `tag:${string}`;

const API = "/filehub/api";

async function getJson<T>(path: string, fallback: T): Promise<T> {
  try {
    const r = await fetch(`${API}${path}`, { cache: "no-store", credentials: "include" });
    if (!r.ok) return fallback;
    return (await r.json()) as T;
  } catch {
    return fallback;
  }
}

function parseTags(raw: string): string[] {
  try {
    const v = JSON.parse(raw || "[]");
    return Array.isArray(v) ? v.map(String) : [];
  } catch {
    return [];
  }
}

function splitTag(t: string): { key: string; value: string } {
  const i = t.indexOf(":");
  return i > 0 ? { key: t.slice(0, i), value: t.slice(i + 1) } : { key: "", value: t };
}

/// Tag folders are saved views with layout "tagfolder" and tag filters.
type TagFolder = { id: string; name: string; tags: string[]; match: "all" | "any"; color: string | null };
function toTagFolder(v: View): TagFolder | null {
  if (v.layout !== "tagfolder") return null;
  let filters: { field: string; op: string; value: string }[] = [];
  try { filters = JSON.parse(v.filters || "[]"); } catch { /* ignore */ }
  const tags = filters.filter((f) => f.field === "tag").map((f) => f.value);
  const match = filters.some((f) => f.op === "all") ? "all" : "any";
  return { id: v.id, name: v.name, tags, match, color: v.color };
}

const STATUS_TONE: Record<string, Tone> = {
  Draft: "slate", Review: "amber", Approved: "emerald", Rejected: "rose", Archived: "violet",
};

function fileIcon(t: string) {
  if (["mp4", "mov", "webm", "mkv"].includes(t)) return "🎬";
  if (["png", "jpg", "jpeg", "gif", "img", "webp"].includes(t)) return "🖼️";
  if (t === "pdf") return "📕";
  if (["xlsx", "xls", "csv"].includes(t)) return "📊";
  if (["docx", "doc", "odt"].includes(t)) return "📝";
  if (["zip", "rar", "7z"].includes(t)) return "🗜️";
  return "📄";
}

// ─────────────────────────────────────────────────────────────────────────────
// Explorer
// ─────────────────────────────────────────────────────────────────────────────

export function Explorer() {
  const sp = useSearchParams();
  const router = useRouter();
  const bucketId = sp.get("b") ?? "";
  const folderId = sp.get("f") ?? "";
  const tagParam = sp.get("tag") ?? "";
  const tagFolderId = sp.get("tf") ?? "";
  const q = sp.get("q") ?? "";
  const group = (sp.get("group") ?? "none") as GroupBy;

  const [me, setMe] = React.useState<Me | null>(null);
  const [systems, setSystems] = React.useState<System[]>([]);
  const [stats, setStats] = React.useState<DashboardStats | null>(null);
  const [folders, setFolders] = React.useState<Folder[]>([]);
  const [tags, setTags] = React.useState<TagRow[]>([]);
  const [views, setViews] = React.useState<View[]>([]);
  const [files, setFiles] = React.useState<FileRow[] | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [modal, setModal] = React.useState<null | "bucket" | "folder" | "tagfolder">(null);
  const [search, setSearch] = React.useState(q);

  React.useEffect(() => setSearch(q), [q]);

  const reloadMeta = React.useCallback(async () => {
    const [m, sys, st, fo, tg, vw] = await Promise.all([
      getJson<Me | null>("/auth/me", null),
      getJson<System[]>("/systems", []),
      getJson<DashboardStats | null>("/stats", null),
      getJson<Folder[]>("/folders", []),
      getJson<TagRow[]>("/tags", []),
      getJson<View[]>("/views", []),
    ]);
    setMe(m); setSystems(sys); setStats(st); setFolders(fo); setTags(tg); setViews(vw);
  }, []);
  React.useEffect(() => { void reloadMeta(); }, [reloadMeta]);

  const buckets = React.useMemo(
    () => systems.filter((s) => s.system_type !== "personal").sort((a, b) => a.name.localeCompare(b.name)),
    [systems],
  );
  const personal = systems.find((s) => s.system_type === "personal");
  const sysById = React.useMemo(() => Object.fromEntries(systems.map((s) => [s.id, s])), [systems]);
  const folderById = React.useMemo(() => Object.fromEntries(folders.map((f) => [f.id, f])), [folders]);
  const tagFolders = React.useMemo(() => views.map(toTagFolder).filter(Boolean) as TagFolder[], [views]);
  const activeTagFolder = tagFolders.find((t) => t.id === tagFolderId) ?? null;
  const usage = React.useMemo(() => {
    const u: Record<string, { files: number; bytes: number }> = {};
    for (const s of stats?.storage_by_system ?? []) u[s.system_id] = { files: s.file_count, bytes: s.size_bytes };
    return u;
  }, [stats]);

  const canEdit = me?.role === "admin" || me?.role === "editor";
  const isAdmin = me?.role === "admin";

  // where are we?
  const mode: "root" | "bucket" | "tag" | "tagfolder" =
    tagFolderId ? "tagfolder" : tagParam ? "tag" : bucketId ? "bucket" : "root";

  // ---- files for the current location ----
  React.useEffect(() => {
    let cancelled = false;
    (async () => {
      setFiles(null);
      let rows: FileRow[] = [];
      const qs = (o: Record<string, string>) => new URLSearchParams({ limit: "500", ...o }).toString();
      if (mode === "bucket") {
        // Searching inside a bucket covers all its folders; browsing shows one folder level.
        const o: Record<string, string> = { system_id: bucketId };
        if (q) o.q = q; else o.folder_id = folderId || "null";
        rows = await getJson<FileRow[]>(`/files?${qs(o)}`, []);
      } else if (mode === "tag") {
        rows = await getJson<FileRow[]>(`/files?${qs(q ? { tag: tagParam, q } : { tag: tagParam })}`, []);
      } else if (mode === "tagfolder" && activeTagFolder) {
        const lists = await Promise.all(activeTagFolder.tags.map((t) =>
          getJson<FileRow[]>(`/files?${qs(q ? { tag: t, q } : { tag: t })}`, [])));
        const byId = new Map<string, { row: FileRow; hits: number }>();
        for (const l of lists) for (const r of l) {
          const e = byId.get(r.id); if (e) e.hits += 1; else byId.set(r.id, { row: r, hits: 1 });
        }
        rows = [...byId.values()]
          .filter((e) => activeTagFolder.match === "any" || e.hits === activeTagFolder.tags.length)
          .map((e) => e.row);
      } else if (mode === "root" && q) {
        rows = await getJson<FileRow[]>(`/files?${qs({ q })}`, []);
      }
      if (!cancelled) setFiles(rows);
    })();
    return () => { cancelled = true; };
  }, [mode, bucketId, folderId, tagParam, q, activeTagFolder]);

  // ---- navigation ----
  const go = (params: Record<string, string | undefined>) => {
    const next = new URLSearchParams();
    for (const [k, v] of Object.entries(params)) if (v) next.set(k, v);
    if (group !== "none" && !("group" in params)) next.set("group", group);
    router.push(`/explorer${next.toString() ? `?${next}` : ""}`);
  };
  const here = { b: bucketId || undefined, f: folderId || undefined, tag: tagParam || undefined, tf: tagFolderId || undefined };

  const folderPath = (id: string): Folder[] => {
    const out: Folder[] = [];
    let cur: Folder | undefined = folderById[id];
    let guard = 0;
    while (cur && guard++ < 50) { out.unshift(cur); cur = cur.parent_id ? folderById[cur.parent_id] : undefined; }
    return out;
  };
  const subfolders = mode === "bucket" && !q
    ? folders.filter((f) => f.system_id === bucketId && (f.parent_id ?? "") === (folderId || ""))
             .sort((a, b) => a.name.localeCompare(b.name))
    : [];

  const locationLabel = (r: FileRow) => {
    const b = sysById[r.system_id]?.name ?? r.system_id;
    const path = r.folder_id ? folderPath(r.folder_id).map((f) => f.name) : [];
    return [b, ...path].join(" › ");
  };
  const showLocation = mode !== "bucket" || !!q;

  // ---- title + breadcrumb ----
  const bucket = sysById[bucketId];
  const crumbs: { label: string; onClick?: () => void }[] = [
    { label: stats?.workspace_display || "Workspace", onClick: mode === "root" && !q ? undefined : () => go({}) },
  ];
  if (mode === "bucket" && bucket) {
    crumbs.push({ label: bucket.name, onClick: folderId || q ? () => go({ b: bucketId }) : undefined });
    const path = folderId ? folderPath(folderId) : [];
    path.forEach((f, i) => crumbs.push({
      label: f.name, onClick: i < path.length - 1 || q ? () => go({ b: bucketId, f: f.id }) : undefined,
    }));
  }
  if (mode === "tag") {
    const { key, value } = splitTag(tagParam);
    crumbs.push({ label: "Tag folders" });
    crumbs.push({ label: key ? `${key}: ${value}` : value });
  }
  if (mode === "tagfolder") {
    crumbs.push({ label: "Tag folders" });
    crumbs.push({ label: activeTagFolder?.name ?? "…" });
  }
  const scopeName = crumbs[crumbs.length - 1]?.label ?? "Workspace";

  // ---- readable filter chips ----
  const chips: { label: string; clear: () => void }[] = [];
  if (q) chips.push({ label: `Search: “${q}”`, clear: () => go({ ...here, q: undefined }) });
  if (group !== "none") chips.push({ label: `Grouped by ${groupLabel(group)}`, clear: () => go({ ...here, q: q || undefined, group: undefined }) });

  // ---- actions ----
  const createFolder = async (name: string) => {
    const r = await fetch(`${API}/folders`, {
      method: "POST", credentials: "include", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, system_id: bucketId, parent_id: folderId || null }),
    });
    if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error ?? r.statusText);
    await reloadMeta();
  };
  const createTagFolder = async (name: string, picked: string[], match: "all" | "any") => {
    const r = await fetch(`${API}/views`, {
      method: "POST", credentials: "include", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        name, layout: "tagfolder", pinned: true,
        filters: picked.map((t) => ({ field: "tag", op: match, value: t })),
      }),
    });
    if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error ?? r.statusText);
    const v = (await r.json()) as View;
    await reloadMeta();
    go({ tf: v.id });
  };
  const deleteTagFolder = async (tf: TagFolder) => {
    if (!confirm(`Delete tag folder “${tf.name}”? Files are not affected.`)) return;
    const r = await mutate(`${API}/views/${encodeURIComponent(tf.id)}`, "DELETE", { credentials: "include" });
    if (!r.ok) { setErr("Could not delete this tag folder."); return; }
    await reloadMeta();
    if (tagFolderId === tf.id) go({});
  };

  const uploadHref = `/upload?system_id=${encodeURIComponent(bucketId)}${folderId ? `&folder_id=${encodeURIComponent(folderId)}` : ""}`;

  return (
    <div className="scr" style={{ height: "100vh" }}>
      <Sidebar nav="files" systems={systems} stats={stats} />
      <TopBar crumbs={["Files"]} />
      <div className="main" style={{ flexDirection: "row", overflow: "hidden" }}>
        <div style={{ display: "flex", minHeight: 0, flex: 1 }}>
          <Tree
            workspace={stats?.workspace_display || "Workspace"}
            workspaceSub={stats?.workspace_name || ""}
            buckets={buckets} personal={personal} usage={usage} folders={folders} tags={tags}
            tagFolders={tagFolders} mode={mode} bucketId={bucketId} folderId={folderId}
            tagParam={tagParam} tagFolderId={tagFolderId}
            canCreateBucket={isAdmin} canCreateTagFolder={!!me}
            onGo={go}
            onNewBucket={() => setModal("bucket")}
            onNewTagFolder={() => setModal("tagfolder")}
            onDeleteTagFolder={deleteTagFolder}
          />

          <div style={{ flex: 1, minWidth: 0, overflow: "auto", padding: "20px 28px 40px" }}>
            {/* breadcrumb */}
            <nav aria-label="Location" style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap", fontSize: 13, marginBottom: 6 }}>
              {crumbs.map((c, i) => (
                <React.Fragment key={i}>
                  {i > 0 && <span className="t-subtle">›</span>}
                  {c.onClick
                    ? <button type="button" className="btn xs ghost" onClick={c.onClick} style={{ padding: "2px 6px" }}>{c.label}</button>
                    : <span className="t-muted" style={{ padding: "2px 6px" }}>{c.label}</span>}
                </React.Fragment>
              ))}
            </nav>

            <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 16, flexWrap: "wrap", marginBottom: 14 }}>
              <div>
                <h1 style={{ fontSize: 22, fontWeight: 650, margin: 0 }}>
                  {mode === "root" ? (stats?.workspace_display || "Workspace") : scopeName}
                </h1>
                <div className="t-sm t-muted" style={{ marginTop: 4 }}>
                  {mode === "root" && "Pick a bucket, or open a tag folder to see files from every bucket that share a tag."}
                  {mode === "bucket" && (bucket?.description || "Folders and files in this bucket.")}
                  {mode === "tag" && "Virtual folder — every file carrying this tag, whichever bucket it lives in."}
                  {mode === "tagfolder" && activeTagFolder &&
                    `Virtual folder — files with ${activeTagFolder.match === "all" ? "all" : "any"} of: ${activeTagFolder.tags.map(prettyTag).join(", ")}`}
                </div>
              </div>
              <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                {mode === "root" && isAdmin && (
                  <button className="btn sm" onClick={() => setModal("bucket")}><Ico.plus className="icon sm" /> New bucket</button>
                )}
                {mode === "bucket" && canEdit && (
                  <>
                    <button className="btn sm" onClick={() => setModal("folder")}><Ico.folder className="icon sm" /> New folder</button>
                    <Link className="btn sm primary" href={uploadHref}><Ico.upload className="icon sm" /> Upload here</Link>
                  </>
                )}
                {(mode === "tag") && me && (
                  <button className="btn sm" onClick={() => setModal("tagfolder")}>Save as tag folder</button>
                )}
              </div>
            </div>

            {/* search + group */}
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 10 }}>
              <form onSubmit={(e) => { e.preventDefault(); go({ ...here, q: search.trim() || undefined }); }}
                    style={{ flex: "1 1 320px", display: "flex", gap: 6 }}>
                <div style={{ position: "relative", flex: 1 }}>
                  <Ico.search className="icon sm" style={{ position: "absolute", left: 10, top: 9, color: "var(--text-subtle)" }} />
                  <input className="field" value={search} onChange={(e) => setSearch(e.target.value)}
                         placeholder={`Search in ${mode === "root" ? "all buckets" : scopeName} — file name or tag`}
                         style={{ width: "100%", paddingLeft: 30 }} aria-label="Search files" />
                </div>
                <button className="btn sm" type="submit">Search</button>
              </form>
              <label className="t-sm t-muted" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                Group by
                <select className="field" value={group}
                        onChange={(e) => go({ ...here, q: q || undefined, group: e.target.value === "none" ? undefined : e.target.value })}>
                  <option value="none">Nothing</option>
                  <option value="type">File type</option>
                  <option value="status">Status</option>
                  <option value="owner">Owner</option>
                  <option value="month">Month modified</option>
                  {uniqueKeys(tags).map((k) => <option key={k} value={`tag:${k}`}>Tag: {k}</option>)}
                </select>
              </label>
            </div>

            {chips.length > 0 && (
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginBottom: 12 }}>
                {chips.map((c, i) => (
                  <span key={i} className="pill indigo" style={{ gap: 6 }}>
                    {c.label}
                    <button type="button" onClick={c.clear} aria-label={`Remove ${c.label}`}
                            style={{ border: 0, background: "transparent", cursor: "pointer", color: "inherit", padding: 0 }}>×</button>
                  </span>
                ))}
              </div>
            )}

            {err && <div role="alert" className="t-sm" style={{ color: "var(--danger)", marginBottom: 10 }}>{err}</div>}

            {/* root: bucket cards */}
            {mode === "root" && !q && (
              <BucketGrid buckets={buckets} personal={personal} usage={usage} onOpen={(id) => go({ b: id })}
                          canCreate={isAdmin} onNew={() => setModal("bucket")} />
            )}

            {/* sub-folders */}
            {subfolders.length > 0 && (
              <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))", gap: 10, marginBottom: 16 }}>
                {subfolders.map((f) => {
                  const kids = folders.filter((x) => x.parent_id === f.id).length;
                  return (
                    <button key={f.id} type="button" className="card" onClick={() => go({ b: bucketId, f: f.id })}
                            style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 12px", textAlign: "left", cursor: "pointer", boxShadow: "none" }}>
                      <span style={{ fontSize: 22 }}>📁</span>
                      <span style={{ minWidth: 0 }}>
                        <span className="t-semibold" style={{ display: "block", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{f.name}</span>
                        <span className="t-xs t-muted">{kids > 0 ? `${kids} sub-folder${kids === 1 ? "" : "s"}` : "Folder"}</span>
                      </span>
                    </button>
                  );
                })}
              </div>
            )}

            {/* files */}
            {(mode !== "root" || q) && (
              files === null
                ? <div className="t-sm t-muted" style={{ padding: 24 }}>Loading…</div>
                : <FileList rows={files} group={group} showLocation={showLocation} locationLabel={locationLabel}
                            onTag={(t) => go({ tag: t })}
                            empty={
                              q ? `No files match “${q}” in ${scopeName}.`
                                : mode === "bucket"
                                  ? (subfolders.length ? "No files directly in this folder." : "This folder is empty — use “Upload here” to add files.")
                                  : "No files carry this tag yet."
                            } />
            )}
          </div>
        </div>
      </div>

      {modal && (
        <Modal onClose={() => setModal(null)}>
          {modal === "bucket" && (
            <BucketForm mode="create" existing={systems}
                        onCancel={() => setModal(null)}
                        onDone={async () => { setModal(null); await reloadMeta(); }}
                        onError={setErr} />
          )}
          {modal === "folder" && (
            <NewFolderForm where={scopeName} onCancel={() => setModal(null)}
                           onCreate={async (n) => { await createFolder(n); setModal(null); }} />
          )}
          {modal === "tagfolder" && (
            <TagFolderForm tags={tags} initial={tagParam ? [tagParam] : []}
                           onCancel={() => setModal(null)}
                           onCreate={async (n, t, m) => { await createTagFolder(n, t, m); setModal(null); }} />
          )}
        </Modal>
      )}
    </div>
  );
}

function prettyTag(t: string) {
  const { key, value } = splitTag(t);
  return key ? `${key}: ${value}` : value;
}
function uniqueKeys(tags: TagRow[]) {
  return [...new Set(tags.map((t) => t.key).filter(Boolean))].sort();
}
function groupLabel(g: GroupBy) {
  if (g.startsWith("tag:")) return `tag “${g.slice(4)}”`;
  return ({ type: "file type", status: "status", owner: "owner", month: "month modified", none: "nothing" } as Record<string, string>)[g] ?? g;
}

// ─────────────────────────────────────────────────────────────────────────────
// Tree (left pane)
// ─────────────────────────────────────────────────────────────────────────────

function Tree(p: {
  workspace: string; workspaceSub: string;
  buckets: System[]; personal?: System; usage: Record<string, { files: number; bytes: number }>;
  folders: Folder[]; tags: TagRow[]; tagFolders: TagFolder[];
  mode: string; bucketId: string; folderId: string; tagParam: string; tagFolderId: string;
  canCreateBucket: boolean; canCreateTagFolder: boolean;
  onGo: (params: Record<string, string | undefined>) => void;
  onNewBucket: () => void; onNewTagFolder: () => void; onDeleteTagFolder: (t: TagFolder) => void;
}) {
  const [open, setOpen] = React.useState<Record<string, boolean>>({ "k:fiscal-year": true });
  // keep the path to the current location expanded
  React.useEffect(() => {
    const next: Record<string, boolean> = {};
    if (p.bucketId) next[`b:${p.bucketId}`] = true;
    let cur = p.folders.find((f) => f.id === p.folderId);
    let guard = 0;
    while (cur && guard++ < 50) { next[`f:${cur.id}`] = true; const parent = cur.parent_id; cur = parent ? p.folders.find((f) => f.id === parent) : undefined; }
    if (p.tagParam) next[`k:${splitTag(p.tagParam).key}`] = true;
    setOpen((o) => ({ ...o, ...next }));
  }, [p.bucketId, p.folderId, p.tagParam, p.folders]);
  const toggle = (k: string) => setOpen((o) => ({ ...o, [k]: !o[k] }));

  const byKey = React.useMemo(() => {
    const m = new Map<string, TagRow[]>();
    for (const t of p.tags) { const k = t.key || "other"; m.set(k, [...(m.get(k) ?? []), t]); }
    return [...m.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [p.tags]);

  const renderFolders = (bucketId: string, parent: string | null, depth: number): React.ReactNode =>
    p.folders.filter((f) => f.system_id === bucketId && (f.parent_id ?? null) === parent)
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((f) => {
        const hasKids = p.folders.some((x) => x.parent_id === f.id);
        const isOpen = !!open[`f:${f.id}`];
        return (
          <React.Fragment key={f.id}>
            <TreeRow depth={depth} active={p.mode === "bucket" && p.folderId === f.id}
                     caret={hasKids ? (isOpen ? "open" : "closed") : "none"} onCaret={() => toggle(`f:${f.id}`)}
                     icon="📁" label={f.name} onClick={() => p.onGo({ b: bucketId, f: f.id })} />
            {hasKids && isOpen && renderFolders(bucketId, f.id, depth + 1)}
          </React.Fragment>
        );
      });

  return (
    <aside aria-label="Explorer" style={{ width: 280, flexShrink: 0, borderRight: "1px solid var(--border)", overflow: "auto", padding: "14px 8px", background: "var(--bg-subtle, transparent)" }}>
      <SectionTitle>Workspace</SectionTitle>
      <TreeRow depth={0} active={p.mode === "root"} caret="none" icon="🏢"
               label={<span><b>{p.workspace}</b>{p.workspaceSub && <span className="t-xs t-muted" style={{ display: "block" }}>{p.workspaceSub}</span>}</span>}
               onClick={() => p.onGo({})} />

      <SectionTitle action={p.canCreateBucket ? <MiniBtn onClick={p.onNewBucket} label="New bucket" /> : null}>
        Buckets · {p.buckets.length}
      </SectionTitle>
      {p.buckets.map((b) => {
        const hasFolders = p.folders.some((f) => f.system_id === b.id);
        const isOpen = !!open[`b:${b.id}`];
        const u = p.usage[b.id];
        return (
          <React.Fragment key={b.id}>
            <TreeRow depth={0} active={p.mode === "bucket" && p.bucketId === b.id && !p.folderId}
                     caret={hasFolders ? (isOpen ? "open" : "closed") : "none"} onCaret={() => toggle(`b:${b.id}`)}
                     icon={<span className={"pill " + b.tone + " sm"} style={{ width: 12, height: 12, padding: 0, justifyContent: "center" }}><span className="dot" /></span>}
                     label={b.name} count={u?.files ? fmtCount(u.files) : undefined}
                     title={`Bucket id: ${b.bucket}${u ? ` · ${fmtCount(u.files)} files · ${fmtBytes(u.bytes)}` : ""}`}
                     onClick={() => p.onGo({ b: b.id })} />
            {hasFolders && isOpen && renderFolders(b.id, null, 1)}
          </React.Fragment>
        );
      })}
      {p.personal && (
        <TreeRow depth={0} active={p.mode === "bucket" && p.bucketId === p.personal.id} caret="none" icon="👤"
                 label="My Drive" count={p.usage[p.personal.id]?.files ? fmtCount(p.usage[p.personal.id].files) : undefined}
                 onClick={() => p.onGo({ b: p.personal!.id })} />
      )}

      <SectionTitle action={p.canCreateTagFolder ? <MiniBtn onClick={p.onNewTagFolder} label="New tag folder" /> : null}>
        Tag folders
      </SectionTitle>
      {p.tagFolders.map((tf) => (
        <TreeRow key={tf.id} depth={0} active={p.mode === "tagfolder" && p.tagFolderId === tf.id} caret="none" icon="🏷️"
                 label={tf.name} onClick={() => p.onGo({ tf: tf.id })}
                 extra={<button type="button" className="tree-x" title="Delete tag folder" aria-label={`Delete ${tf.name}`}
                                onClick={(e) => { e.stopPropagation(); p.onDeleteTagFolder(tf); }}>×</button>} />
      ))}
      {p.tagFolders.length > 0 && <div style={{ height: 6 }} />}
      {byKey.length === 0 && <div className="t-xs t-subtle" style={{ padding: "4px 12px" }}>No tags yet.</div>}
      {byKey.map(([key, rows]) => {
        const isOpen = !!open[`k:${key}`];
        const total = rows.reduce((n, r) => n + r.count, 0);
        return (
          <React.Fragment key={key}>
            <TreeRow depth={0} active={false} caret={isOpen ? "open" : "closed"} onCaret={() => toggle(`k:${key}`)}
                     icon="🗂️" label={key} count={fmtCount(total)} onClick={() => toggle(`k:${key}`)} />
            {isOpen && rows.sort((a, b) => a.value.localeCompare(b.value)).map((r) => (
              <TreeRow key={r.tag} depth={1} active={p.mode === "tag" && p.tagParam === r.tag} caret="none" icon="🏷️"
                       label={r.value} count={fmtCount(r.count)} onClick={() => p.onGo({ tag: r.tag })} />
            ))}
          </React.Fragment>
        );
      })}
    </aside>
  );
}

function SectionTitle({ children, action }: { children: React.ReactNode; action?: React.ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "14px 10px 4px", fontSize: 11, fontWeight: 600, letterSpacing: 0.4, textTransform: "uppercase", color: "var(--text-subtle)" }}>
      <span>{children}</span>{action}
    </div>
  );
}

function MiniBtn({ onClick, label }: { onClick: () => void; label: string }) {
  return (
    <button type="button" onClick={onClick} title={label} aria-label={label} className="btn xs ghost" style={{ padding: "0 6px", textTransform: "none", letterSpacing: 0 }}>
      <Ico.plus className="icon sm" />
    </button>
  );
}

function TreeRow({ depth, active, caret, onCaret, icon, label, count, onClick, title, extra }: {
  depth: number; active: boolean; caret: "open" | "closed" | "none"; onCaret?: () => void;
  icon: React.ReactNode; label: React.ReactNode; count?: string; onClick: () => void; title?: string; extra?: React.ReactNode;
}) {
  return (
    <div role="treeitem" aria-selected={active} tabIndex={0} title={title}
         onClick={onClick} onKeyDown={(e) => { if (e.key === "Enter") onClick(); }}
         className={"side-row" + (active ? " active" : "")}
         style={{ paddingLeft: 6 + depth * 16, cursor: "pointer", gap: 6 }}>
      <span onClick={(e) => { if (caret !== "none" && onCaret) { e.stopPropagation(); onCaret(); } }}
            style={{ width: 14, display: "inline-flex", justifyContent: "center", color: "var(--text-subtle)" }}
            aria-hidden="true">
        {caret === "open" ? "▾" : caret === "closed" ? "▸" : ""}
      </span>
      <span style={{ width: 18, display: "inline-flex", justifyContent: "center" }}>{icon}</span>
      <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</span>
      {count && <span className="count">{count}</span>}
      {extra}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Content
// ─────────────────────────────────────────────────────────────────────────────

function BucketGrid({ buckets, personal, usage, onOpen, canCreate, onNew }: {
  buckets: System[]; personal?: System; usage: Record<string, { files: number; bytes: number }>;
  onOpen: (id: string) => void; canCreate: boolean; onNew: () => void;
}) {
  const all = personal ? [...buckets, personal] : buckets;
  return (
    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(240px, 1fr))", gap: 12 }}>
      {all.map((b) => {
        const u = usage[b.id];
        const quota = b.quota_bytes ?? 0;
        const pct = quota > 0 && u ? Math.min(100, (u.bytes / quota) * 100) : 0;
        return (
          <button key={b.id} type="button" className="card" onClick={() => onOpen(b.id)}
                  style={{ textAlign: "left", padding: 14, cursor: "pointer", display: "flex", flexDirection: "column", gap: 8, boxShadow: "none" }}>
            <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ fontSize: 20 }}>{b.system_type === "personal" ? "👤" : "🪣"}</span>
              <span className="t-semibold" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {b.system_type === "personal" ? "My Drive" : b.name}
              </span>
            </span>
            {b.description && <span className="t-xs t-muted" style={{ minHeight: 16 }}>{b.description}</span>}
            <span className="t-xs t-tabular">
              <b>{u?.files ? `${fmtCount(u.files)} file${u.files === 1 ? "" : "s"}` : "Empty"}</b>
              {" · "}{fmtBytes(u?.bytes ?? 0)}{quota > 0 ? ` of ${fmtBytes(quota)}` : ""}
            </span>
            {quota > 0 && (
              <span style={{ height: 5, borderRadius: 999, background: "var(--bg-muted)", overflow: "hidden", display: "block" }}>
                <span style={{ display: "block", width: `${pct}%`, height: "100%", background: pct >= 90 ? "var(--danger)" : "var(--accent)" }} />
              </span>
            )}
          </button>
        );
      })}
      {canCreate && (
        <button type="button" className="card" onClick={onNew}
                style={{ padding: 14, cursor: "pointer", borderStyle: "dashed", color: "var(--text-muted)", boxShadow: "none", display: "flex", alignItems: "center", justifyContent: "center", gap: 6 }}>
          <Ico.plus className="icon sm" /> New bucket
        </button>
      )}
    </div>
  );
}

function groupKey(r: FileRow, g: GroupBy): string {
  if (g === "type") return r.file_type.toUpperCase();
  if (g === "status") return r.status;
  if (g === "owner") return r.owner || "Unknown";
  if (g === "month") return new Date(r.modified_at).toLocaleDateString("en-GB", { month: "long", year: "numeric" });
  if (g.startsWith("tag:")) {
    const key = g.slice(4);
    const hit = parseTags(r.tags).map(splitTag).find((t) => t.key === key);
    return hit ? hit.value : `(no ${key})`;
  }
  return "";
}

function FileList({ rows, group, showLocation, locationLabel, onTag, empty }: {
  rows: FileRow[]; group: GroupBy; showLocation: boolean; locationLabel: (r: FileRow) => string;
  onTag: (t: string) => void; empty: string;
}) {
  if (rows.length === 0) {
    return <div className="card t-sm t-muted" style={{ padding: 28, textAlign: "center", boxShadow: "none" }}>{empty}</div>;
  }
  const groups = new Map<string, FileRow[]>();
  for (const r of rows) {
    const k = group === "none" ? "" : groupKey(r, group);
    groups.set(k, [...(groups.get(k) ?? []), r]);
  }
  const ordered = [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  return (
    <div className="card" style={{ padding: 0, overflow: "hidden", boxShadow: "none" }}>
      <table className="tbl" style={{ width: "100%" }}>
        <thead>
          <tr>
            <th>Name</th>
            {showLocation && <th>Location</th>}
            <th style={{ width: 110 }}>Status</th>
            <th>Tags</th>
            <th style={{ width: 90, textAlign: "right" }}>Size</th>
            <th style={{ width: 120 }}>Modified</th>
          </tr>
        </thead>
        <tbody>
          {ordered.map(([k, list]) => (
            <React.Fragment key={k || "_"}>
              {group !== "none" && (
                <tr>
                  <td colSpan={showLocation ? 6 : 5} style={{ background: "var(--bg-muted)", fontWeight: 600, fontSize: 12 }}>
                    📂 {k} <span className="t-muted" style={{ fontWeight: 400 }}>· {list.length}</span>
                  </td>
                </tr>
              )}
              {list.map((r) => (
                <tr key={r.id}>
                  <td style={{ maxWidth: 360 }}>
                    <Link href={`/files/${r.id}`} style={{ display: "flex", alignItems: "center", gap: 8, color: "inherit" }}>
                      <span aria-hidden="true">{fileIcon(r.file_type)}</span>
                      <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.name}</span>
                    </Link>
                  </td>
                  {showLocation && <td className="t-xs t-muted">{locationLabel(r)}</td>}
                  <td><Pill tone={STATUS_TONE[r.status] ?? "slate"} sm><span className="dot" />{r.status}</Pill></td>
                  <td>
                    <span style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
                      {parseTags(r.tags).filter((t) => !t.startsWith("system:") && !t.startsWith("uploaded-by:")).slice(0, 3).map((t) => (
                        <button key={t} type="button" className="tag" onClick={() => onTag(t)} title={`Open tag folder “${prettyTag(t)}”`}
                                style={{ cursor: "pointer", border: 0 }}>
                          {prettyTag(t)}
                        </button>
                      ))}
                    </span>
                  </td>
                  <td className="t-tabular t-sm" style={{ textAlign: "right" }}>{fmtBytes(r.size_bytes)}</td>
                  <td className="t-sm t-muted">{fmtAgo(r.modified_at)}</td>
                </tr>
              ))}
            </React.Fragment>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Modals
// ─────────────────────────────────────────────────────────────────────────────

function Modal({ children, onClose }: { children: React.ReactNode; onClose: () => void }) {
  React.useEffect(() => {
    const k = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div role="dialog" aria-modal="true" onClick={onClose}
         style={{ position: "fixed", inset: 0, background: "rgba(15,23,42,.35)", display: "flex", alignItems: "flex-start", justifyContent: "center", padding: "10vh 16px", zIndex: 50 }}>
      <div onClick={(e) => e.stopPropagation()} style={{ width: "min(640px, 100%)", background: "var(--bg)", borderRadius: 12, boxShadow: "0 20px 50px rgba(0,0,0,.25)" }}>
        {children}
      </div>
    </div>
  );
}

function NewFolderForm({ where, onCreate, onCancel }: { where: string; onCreate: (n: string) => Promise<void>; onCancel: () => void }) {
  const [name, setName] = React.useState("");
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  return (
    <form style={{ padding: 20 }} onSubmit={async (e) => {
      e.preventDefault();
      if (!name.trim()) { setErr("Please enter a folder name."); return; }
      setBusy(true); setErr(null);
      try { await onCreate(name.trim()); } catch (x) { setErr(String((x as Error).message)); } finally { setBusy(false); }
    }}>
      <div className="t-md t-semibold">New folder</div>
      <div className="t-xs t-muted" style={{ margin: "2px 0 12px" }}>Inside “{where}”</div>
      <input className="field" autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Folder name" style={{ width: "100%" }} />
      {err && <div className="t-xs" style={{ color: "var(--danger)", marginTop: 6 }}>{err}</div>}
      <div style={{ display: "flex", gap: 8, marginTop: 14 }}>
        <button className="btn sm primary" type="submit" disabled={busy}>{busy ? "Creating…" : "Create folder"}</button>
        <button className="btn sm ghost" type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}

function TagFolderForm({ tags, initial, onCreate, onCancel }: {
  tags: TagRow[]; initial: string[]; onCreate: (n: string, t: string[], m: "all" | "any") => Promise<void>; onCancel: () => void;
}) {
  const [picked, setPicked] = React.useState<string[]>(initial);
  const [name, setName] = React.useState(initial[0] ? prettyTag(initial[0]) : "");
  const [match, setMatch] = React.useState<"all" | "any">("all");
  const [filter, setFilter] = React.useState("");
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const shown = tags.filter((t) => !filter || t.tag.toLowerCase().includes(filter.toLowerCase()));
  const toggle = (t: string) => setPicked((p) => p.includes(t) ? p.filter((x) => x !== t) : [...p, t]);
  return (
    <form style={{ padding: 20 }} onSubmit={async (e) => {
      e.preventDefault();
      if (!name.trim()) { setErr("Please give the tag folder a name."); return; }
      if (picked.length === 0) { setErr("Pick at least one tag."); return; }
      setBusy(true); setErr(null);
      try { await onCreate(name.trim(), picked, match); } catch (x) { setErr(String((x as Error).message)); } finally { setBusy(false); }
    }}>
      <div className="t-md t-semibold">New tag folder</div>
      <div className="t-xs t-muted" style={{ margin: "2px 0 12px" }}>
        A virtual folder that collects files by tag from every bucket. Files are not moved or copied.
      </div>
      <input className="field" value={name} onChange={(e) => setName(e.target.value)} placeholder="Folder name, e.g. Loan decree 2569" style={{ width: "100%", marginBottom: 10 }} />
      <div style={{ display: "flex", gap: 12, alignItems: "center", marginBottom: 8, fontSize: 13 }}>
        <span className="t-muted">Show files that have</span>
        <label><input type="radio" checked={match === "all"} onChange={() => setMatch("all")} /> all selected tags</label>
        <label><input type="radio" checked={match === "any"} onChange={() => setMatch("any")} /> any of them</label>
      </div>
      <input className="field" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter tags…" style={{ width: "100%", marginBottom: 6 }} />
      <div style={{ maxHeight: 220, overflow: "auto", border: "1px solid var(--border)", borderRadius: 8, padding: 6 }}>
        {shown.map((t) => (
          <label key={t.tag} style={{ display: "flex", alignItems: "center", gap: 8, padding: "3px 4px", fontSize: 13 }}>
            <input type="checkbox" checked={picked.includes(t.tag)} onChange={() => toggle(t.tag)} />
            <span style={{ flex: 1 }}>{prettyTag(t.tag)}</span>
            <span className="t-xs t-muted">{fmtCount(t.count)}</span>
          </label>
        ))}
        {shown.length === 0 && <div className="t-xs t-muted" style={{ padding: 6 }}>No tags match.</div>}
      </div>
      {err && <div className="t-xs" style={{ color: "var(--danger)", marginTop: 6 }}>{err}</div>}
      <div style={{ display: "flex", gap: 8, marginTop: 14 }}>
        <button className="btn sm primary" type="submit" disabled={busy}>{busy ? "Creating…" : "Create tag folder"}</button>
        <button className="btn sm ghost" type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}
