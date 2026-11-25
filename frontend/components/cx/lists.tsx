"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";

import { api, bytes, messageFor, BASE_PATH, type Basket, type DlEntry, type DlItem, type FileRow, type Settings } from "@/lib/fh";
import { loadSettings } from "./basket-view";
import { useNav } from "./shell";
import { Btn, Dropdown, Empty, FileTile, I, Note, Pager, SkeletonRows, StatusTag, TextField, useToast, usePageSize, When } from "./ui";

type Row = {
  kind: "file" | "folder"; id: string; name: string; system_id: string; basket_name?: string; folder_id: string | null;
  status?: string | null; size_bytes?: number | null; modified_at?: string | null; level?: number; granted_by?: string | null;
  owner?: string | null; version?: number;
};

/// GET a list and the server's total (x-total-count) for paging.
async function apiPage<T>(path: string): Promise<{ rows: T[]; total: number | null }> {
  const r = await fetch(`${BASE_PATH}/api${path}`, { cache: "no-store", credentials: "same-origin" });
  const text = await r.text();
  let data: unknown = null;
  try { data = text ? JSON.parse(text) : null; } catch { data = text; }
  if (!r.ok) throw new Error(messageFor(r.status, data));
  const t = r.headers.get("x-total-count");
  return { rows: data as T[], total: t == null ? null : Number(t) };
}

/// Recent · Shared with me · Awaiting my review · Search results — one table shape, paged.
export function FileList({ kind, q, filters = "" }: { kind: "recent" | "shared" | "review" | "search"; q?: string; filters?: string }) {
  const nav = useNav();
  const router = useRouter();
  const [rows, setRows] = React.useState<Row[] | null>(null);
  const [total, setTotal] = React.useState<number | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [statuses, setStatuses] = React.useState<Settings["statuses"]>([]);
  const [page, setPage] = React.useState(0);
  const [size, setSize] = usePageSize(kind === "search" ? "search" : "recent");
  React.useEffect(() => { setPage(0); }, [kind, q, filters, size]);

  React.useEffect(() => {
    let off = false;
    setRows(null); setErr(null);
    const names = new Map<string, string>([...nav.baskets.map((b): [string, string] => [b.id, b.name]), ...(nav.drive ? [[nav.drive.id, "My Drive"] as [string, string]] : [])]);
    const off0 = page * size;
    const load = async (): Promise<{ rows: Row[]; total: number }> => {
      if (kind === "shared" || kind === "review") {
        // These lists come whole; page them here.
        const all = await api<Row[]>(kind === "shared" ? "/me/shared" : "/me/review");
        return { rows: all.slice(off0, off0 + size), total: all.length };
      }
      const res = kind === "search"
        ? await apiPage<FileRow>(`/search?q=${encodeURIComponent(q ?? "")}&limit=${size}&offset=${off0}${filters}`)
        : await apiPage<FileRow>(`/files?sort=modified&dir=desc&limit=${size}&offset=${off0}`);
      // Folders match by name too (not by file type); they head the first page.
      const folders: Row[] = kind === "search" && q && page === 0 && !/[?&]types=/.test(filters)
        ? (await api<{ id: string; system_id: string; parent_id: string | null; name: string; created_at: string }[]>(`/search/folders?q=${encodeURIComponent(q)}${filters}`))
          .map((d) => ({ kind: "folder", id: d.id, name: d.name, system_id: d.system_id, basket_name: names.get(d.system_id) ?? d.system_id, folder_id: d.parent_id, modified_at: d.created_at }))
        : [];
      return { rows: [...folders, ...res.rows.map((f) => ({ kind: "file", id: f.id, name: f.name, system_id: f.system_id, basket_name: names.get(f.system_id) ?? "Personal drive",
        folder_id: f.folder_id, status: f.status, size_bytes: f.size_bytes, modified_at: f.modified_at, owner: f.owner, version: f.version } as Row))], total: res.total ?? res.rows.length };
    };
    load().then(async ({ rows: r, total: t }) => {
      if (off) return;
      setRows(r); setTotal(t);
      const ids = [...new Set(r.map((x) => x.system_id))];
      const ss = await Promise.all(ids.map((i) => loadSettings(i)));
      const m = new Map<string, Settings["statuses"][number]>();
      for (const s of ss) for (const st of s?.statuses ?? []) if (!m.has(st.code)) m.set(st.code, st);
      if (!off) setStatuses([...m.values()]);
    }).catch((e) => { if (!off) { setErr(e.message); setRows([]); setTotal(0); } });
    return () => { off = true; };
  }, [kind, q, filters, nav.baskets, nav.drive, page, size]);

  const title = { recent: "Recent", shared: "Shared with me", review: "Awaiting my review", search: q ? `Results for “${q}”` : "Search results" }[kind];
  const empty = {
    recent: "No files yet", shared: "Nothing has been shared with you",
    review: "Nothing is waiting for your review", search: "No files or folders match",
  }[kind];
  const open = (r: Row) => r.kind === "folder"
    ? router.push(`/b/${r.system_id}?folder=${r.id}`)
    : router.push(`/b/${r.system_id}?${r.folder_id ? `folder=${r.folder_id}&` : ""}file=${r.id}`);
  return (
    <div className="cx-page">
      <div className="cx-ph" style={{ marginTop: 0 }}>
        <div><h1 className="cx-title">{title}</h1><div className="cx-sub cx-num">{total != null ? `${total} item${total === 1 ? "" : "s"}` : ""}</div></div>
      </div>
      {kind === "search" && <SearchBox initial={q ?? ""} />}
      {kind === "search" && <SearchFilters />}
      {err && <Note kind="error">{err}</Note>}
      <div style={{ border: "1px solid var(--cx-border-subtle)" }}>
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr>
            <th>Name</th><th className="cx-hide-sm cx-hide-md">Basket</th><th>Status</th>
            {kind === "shared" && <th className="cx-hide-sm">Shared by</th>}
            <th className="cx-hide-sm">Modified</th><th className="cx-right cx-hide-sm cx-hide-md">Size</th>
          </tr></thead>
          <tbody>
            {!rows && <SkeletonRows cols={kind === "shared" ? 6 : 5} />}
            {rows?.map((r) => (
              <tr key={`${r.kind}-${r.id}`} onDoubleClick={() => open(r)}>
                <td className="cx-name"><button className="cx-fname" onClick={() => open(r)}>
                  {r.kind === "folder" ? <I.folder size={20} className="cx-folder-ic" /> : <FileTile name={r.name} />}
                  <span><span className="cx-fname-t">{r.name}</span><span className="cx-fname-meta cx-num">{r.version && r.version > 1 ? `Version ${r.version} · ` : ""}{r.owner ?? ""}</span></span></button></td>
                <td className="cx-hide-sm cx-hide-md"><Link href={`/b/${r.system_id}`} style={{ color: "inherit" }}>{r.basket_name}</Link></td>
                <td>{r.status ? <StatusTag code={r.status} statuses={statuses} /> : null}</td>
                {kind === "shared" && <td className="cx-hide-sm">{r.granted_by ?? ""}</td>}
                <td className="cx-hide-sm"><When iso={r.modified_at} /></td>
                <td className="cx-right cx-num cx-hide-sm cx-hide-md">{bytes(r.size_bytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {rows && rows.length === 0 && !err && <Empty icon={kind === "review" ? <I.review size={20} /> : kind === "shared" ? <I.share size={20} /> : kind === "search" ? <I.search size={20} /> : <I.recent size={20} />}>{empty}</Empty>}
      </div>
      {rows && (total ?? 0) > 0 && <Pager page={page} size={size} total={total} onPage={setPage} onSize={setSize} />}
      </div>
    </div>
  );
}

function SearchBox({ initial }: { initial: string }) {
  const router = useRouter();
  const [v, setV] = React.useState(initial);
  return (
    <form className="cx-toolbar" onSubmit={(e) => { e.preventDefault(); const p = new URLSearchParams(window.location.search); p.set("q", v.trim()); router.push(`/search?${p}`); }}>
      <div className="cx-grow cx-search-wrap"><I.search />
        <input className="cx-search" value={v} onChange={(e) => setV(e.target.value)} placeholder="Search file names, folders, owners, tags, or document text" aria-label="Search" /></div>
      <Btn kind="primary" type="submit">Search</Btn>
    </form>
  );
}

const TYPE_GROUPS: { value: string; label: string; types: string }[] = [
  { value: "", label: "All file types", types: "" },
  { value: "pdf", label: "PDF", types: "pdf" },
  { value: "word", label: "Word", types: "doc,docx" },
  { value: "excel", label: "Excel", types: "xls,xlsx,csv" },
  { value: "ppt", label: "PowerPoint", types: "ppt,pptx" },
  { value: "image", label: "Images", types: "png,jpg,jpeg,gif,img,webp" },
  { value: "zip", label: "Archives", types: "zip,rar,7z" },
  { value: "text", label: "Text", types: "txt" },
];

/// Search filters: file type, creator, created between. Kept in the URL.
function SearchFilters() {
  const router = useRouter();
  const p = typeof window !== "undefined" ? new URLSearchParams(window.location.search) : new URLSearchParams();
  const [type, setType] = React.useState(p.get("type") ?? "");
  const [creator, setCreator] = React.useState(p.get("creator") ?? "");
  const [from, setFrom] = React.useState(p.get("from") ?? "");
  const [to, setTo] = React.useState(p.get("to") ?? "");
  const apply = (t = type) => {
    const n = new URLSearchParams(window.location.search);
    const put = (k: string, v: string) => { if (v) n.set(k, v); else n.delete(k); };
    put("type", t); put("creator", creator.trim()); put("from", from); put("to", to);
    router.push(`/search?${n}`);
  };
  return (
    <form className="cx-toolbar" onSubmit={(e) => { e.preventDefault(); apply(); }} style={{ flexWrap: "wrap", alignItems: "flex-end" }}>
      <Dropdown label="File type" value={type} options={TYPE_GROUPS.map(({ value, label }) => ({ value, label }))} onChange={(v) => { setType(v); apply(v); }} />
      <TextField label="Created by" value={creator} onChange={(e) => setCreator(e.target.value)} />
      <TextField label="Created from" type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
      <TextField label="Created to" type="date" value={to} onChange={(e) => setTo(e.target.value)} />
      <Btn kind="tertiary" type="submit">Apply filters</Btn>
    </form>
  );
}

/// The API query part for the filters in the page URL.
export function searchFilterQuery(sp: URLSearchParams): string {
  const out = new URLSearchParams();
  const t = TYPE_GROUPS.find((g) => g.value === sp.get("type"))?.types;
  if (t) out.set("types", t);
  for (const k of ["creator", "from", "to"]) { const v = sp.get(k); if (v) out.set(k, v); }
  const s = out.toString();
  return s ? `&${s}` : "";
}

/// Download list → one zip.
export function DownloadList() {
  const nav = useNav();
  const toast = useToast();
  const [rows, setRows] = React.useState<DlEntry[] | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const load = React.useCallback(() => api<DlEntry[]>("/me/downloads").then((r) => { setRows(r); nav.setDownloads(r.length); }).catch((e) => setErr(e.message)), [nav]);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  React.useEffect(() => { load(); }, []);
  const save = async (items: DlItem[]) => { const r = await api<DlEntry[]>("/me/downloads", { method: "PUT", json: items }); setRows(r); nav.setDownloads(r.length); };
  const total = rows?.reduce((a, r) => a + r.files, 0) ?? 0;
  const zip = async () => {
    setBusy(true); setErr(null);
    try {
      const r = await fetch(`${BASE_PATH}/api/me/downloads/zip`, { method: "POST" });
      if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error ?? `Download failed (${r.status}).`);
      const blob = await r.blob();
      const name = /filename="([^"]+)"/.exec(r.headers.get("content-disposition") ?? "")?.[1] ?? "filehub.zip";
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob); a.download = name; a.click();
      setTimeout(() => URL.revokeObjectURL(a.href), 10000);
      toast({ text: `Downloaded ${name}.` });
    } catch (e) { setErr((e as Error).message); }
    setBusy(false);
  };
  return (
    <div className="cx-page" style={{ maxWidth: 960 }}>
      <div className="cx-crumbs" />
      <div className="cx-title-row">
        <h1 className="cx-title">Download list</h1>
        <span className="cx-sub cx-num">{rows ? `${rows.length} item${rows.length === 1 ? "" : "s"} · ${total} file${total === 1 ? "" : "s"}` : ""}</span>
        <span className="cx-spacer" />
        {rows && rows.length > 0 && <Btn kind="ghost" onClick={() => save([])}>Clear list</Btn>}
        <Btn kind="primary" icon={<I.download />} disabled={!rows?.length || busy} onClick={zip}>{busy ? "Preparing zip…" : "Download zip"}</Btn>
      </div>
      {err && <Note kind="error">{err}</Note>}
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>Item</th><th>Type</th><th className="cx-right">Files</th><th style={{ width: 32 }} /></tr></thead>
          <tbody>
            {!rows && <SkeletonRows cols={4} rows={3} />}
            {rows?.map((r, i) => (
              <tr key={i}>
                <td className="cx-name"><span className="cx-fname" style={{ cursor: "default" }}>{r.item.kind === "vgroup" ? <I.filter size={20} className="cx-folder-ic" /> : r.item.kind === "folder" ? <I.folder size={20} className="cx-folder-ic" /> : <FileTile name={r.label} />}<span>{r.label}</span></span></td>
                <td>{r.item.kind === "vgroup" ? "Virtual basket" : r.item.kind === "folder" ? "Folder" : "File"}</td>
                <td className="cx-right cx-num">{r.files}</td>
                <td><button className="cx-close" aria-label={`Remove ${r.label}`} onClick={() => save(rows.filter((_, j) => j !== i).map((x) => x.item))}><I.close /></button></td>
              </tr>
            ))}
          </tbody>
        </table>
        {rows && rows.length === 0 && <Empty icon={<I.download size={20} />}>Your download list is empty</Empty>}
      </div>
    </div>
  );
}

/// Redirect helpers for shared links (/f/:id, /folder/:id).
export function GoToFile({ id }: { id: string }) {
  const router = useRouter();
  const [err, setErr] = React.useState<string | null>(null);
  React.useEffect(() => {
    api<FileRow>(`/files/${id}`).then((f) => router.replace(`/b/${f.system_id}?${f.folder_id ? `folder=${f.folder_id}&` : ""}file=${f.id}`)).catch((e) => setErr(e.message));
  }, [id, router]);
  return <div className="cx-page">{err ? <Note kind="error">{err}</Note> : <span className="cx-skel" style={{ width: 200 }} />}</div>;
}
export function GoToFolder({ id }: { id: string }) {
  const router = useRouter();
  const [err, setErr] = React.useState<string | null>(null);
  React.useEffect(() => {
    api<{ id: string; system_id: string }>(`/folders/${id}`).then((f) => router.replace(`/b/${f.system_id}?folder=${f.id}`)).catch((e) => setErr(e.message));
  }, [id, router]);
  return <div className="cx-page">{err ? <Note kind="error">{err}</Note> : <span className="cx-skel" style={{ width: 200 }} />}</div>;
}
