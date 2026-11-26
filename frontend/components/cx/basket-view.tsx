"use client";

import * as React from "react";
import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";

import { useAuth } from "@/lib/auth-context";
import {
  api, bytes, meta, when, BASE_PATH,
  type Basket, type DlItem, type FileRow, type Folder, type Settings,
} from "@/lib/fh";
import { FilePanel } from "./file-panel";
import { FolderDetailsModal, MoveModal, NameModal, NewFolderModal, ShareModal, StatusModal, UploadModal } from "./modals";
import { FolderTree } from "./folder-tree";
import { BasketAccess, BasketActivity } from "./basket-tabs";
import { BasketSettings } from "./admin";
import { useNav } from "./shell";
import {
  basketColor, Btn, Confirm, Dropdown, Empty, FileTile, I, Note, OverflowMenu, Pager, SkeletonRows, StatusTag, Tag, useToast, usePageSize,
} from "./ui";

const settingsCache = new Map<string, Promise<Settings | null>>();
export function loadSettings(basket: string, fresh = false): Promise<Settings | null> {
  if (fresh || !settingsCache.has(basket)) settingsCache.set(basket, api<Settings>(`/baskets/${basket}/settings`).catch(() => null));
  return settingsCache.get(basket)!;
}

type Sel = { kind: "file" | "folder"; id: string };
type SortKey = "name" | "modified" | "size" | "status" | "owner";
type PageTab = "files" | "access" | "activity" | "settings";

const AV = ["#1192e8", "#198038", "#8a3ffc", "#b28600", "#007d79", "#d02670", "#8a3800"];
/// Avatar color picked from the name, so one person keeps one color.
export function avatarColor(name: string): string {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return AV[h % AV.length];
}
export function Who({ name }: { name: string }) {
  if (!name) return null;
  return <span className="cx-who"><span className="cx-who-av" style={{ background: avatarColor(name) }}>{name.trim().charAt(0).toUpperCase()}</span>{name}</span>;
}

/// FH-01 Basket page: header (breadcrumb · title · sub line · actions), tabs
/// Files · Access · Activity · Settings, and the Files explorer (folder tree +
/// file table + details panel). Columns come from the basket's settings.
export function BasketView({ basketId }: { basketId: string }) {
  const router = useRouter();
  const pathname = usePathname();
  const sp = useSearchParams();
  const folderId = sp.get("folder");
  const fileParam = sp.get("file");
  const nav = useNav();
  const { user } = useAuth();
  const toast = useToast();

  const [basket, setBasket] = React.useState<Basket | null>(null);
  const [settings, setSettings] = React.useState<Settings | null>(null);
  const [folders, setFolders] = React.useState<Folder[] | null>(null);
  const [files, setFiles] = React.useState<FileRow[] | null>(null);
  const [total, setTotal] = React.useState(0);
  const [usage, setUsage] = React.useState<{ used: number; cap: number } | null>(null);
  const [level, setLevel] = React.useState<string>("none");
  const [err, setErr] = React.useState<string | null>(null);
  const [q, setQ] = React.useState("");
  const [qDeb, setQDeb] = React.useState("");
  const [cat, setCat] = React.useState("");
  const [status, setStatus] = React.useState("");
  const [sort, setSort] = React.useState<{ key: SortKey; dir: "asc" | "desc" }>({ key: "modified", dir: "desc" });
  const [page, setPage] = React.useState(0);
  const [size, setSize] = usePageSize("files");
  const [sel, setSel] = React.useState<Sel[]>([]);
  const [tick, setTick] = React.useState(0);
  const [modal, setModal] = React.useState<React.ReactNode>(null);
  const [dropping, setDropping] = React.useState(false);
  const refresh = React.useCallback(() => setTick((t) => t + 1), []);

  React.useEffect(() => { const t = setTimeout(() => setQDeb(q), 250); return () => clearTimeout(t); }, [q]);
  React.useEffect(() => { setSel([]); setPage(0); }, [basketId, folderId]);
  React.useEffect(() => { setPage(0); }, [qDeb, status, cat, sort, size]);

  // Basket, settings, every folder of the basket (breadcrumb + tree), access level.
  React.useEffect(() => {
    let off = false;
    setErr(null);
    (async () => {
      try {
        const [all, me, s, fl, eff] = await Promise.all([
          api<Basket[]>("/systems"), api<Basket>("/personal-drive").catch(() => null), loadSettings(basketId, true),
          api<Folder[]>(`/folders?system_id=${encodeURIComponent(basketId)}`),
          api<{ level: string }>(`/acl/${folderId ? "folder" : "basket"}/${folderId ?? basketId}/effective`).catch(() => ({ level: "none" })),
        ]);
        const b = all.find((x) => x.id === basketId) ?? (me?.id === basketId ? me : null);
        if (off) return;
        if (!b) { setErr("This basket doesn't exist or you don't have access to it."); return; }
        setBasket(b); setSettings(s); setFolders(fl); setLevel(eff.level);
      } catch (e) { if (!off) setErr((e as Error).message); }
    })();
    return () => { off = true; };
  }, [basketId, folderId, tick]);

  // Basket size against its quota (sub line).
  React.useEffect(() => {
    api<{ scopes: { scope: string; used_bytes: number; limit_bytes: number }[] }>(`/quota?system_id=${encodeURIComponent(basketId)}`)
      .then((r) => { const x = r.scopes.find((s) => s.scope === "system"); setUsage(x ? { used: x.used_bytes, cap: x.limit_bytes } : null); })
      .catch(() => setUsage(null));
  }, [basketId, tick]);

  React.useEffect(() => {
    let off = false;
    setFiles(null);
    const p = new URLSearchParams({ system_id: basketId, folder_id: folderId ?? "null", sort: sort.key, dir: sort.dir, limit: String(size), offset: String(page * size) });
    if (qDeb) p.set("q", qDeb);
    if (status) p.set("status", status);
    if (cat) p.set("category", cat);
    fetch(`${BASE_PATH}/api/files?${p}`, { cache: "no-store" }).then(async (r) => {
      if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error ?? `Couldn't load files (${r.status}).`);
      const rows = (await r.json()) as FileRow[];
      if (!off) { setFiles(rows); setTotal(Number(r.headers.get("x-total-count") ?? rows.length)); }
    }).catch((e) => { if (!off) { setFiles([]); setErr(e.message); } });
    return () => { off = true; };
  }, [basketId, folderId, qDeb, status, cat, sort, page, size, tick]);

  const chain = React.useMemo(() => {
    const out: Folder[] = [];
    let cur = folders?.find((f) => f.id === folderId) ?? null;
    let guard = 0;
    while (cur && guard++ < 64) { out.unshift(cur); cur = folders?.find((f) => f.id === cur!.parent_id) ?? null; }
    return out;
  }, [folders, folderId]);
  const here = chain[chain.length - 1];
  const allKids = React.useMemo(() => {
    const list = (folders ?? []).filter((f) => (f.parent_id ?? null) === (folderId ?? null));
    const fq = qDeb.toLowerCase();
    return (fq ? list.filter((f) => f.name.toLowerCase().includes(fq)) : list).sort((a, b) => a.name.localeCompare(b.name));
  }, [folders, folderId, qDeb]);
  // Folders sit above the files on the first page only.
  const kids = page === 0 ? allKids : [];
  const listFields = (settings?.fields ?? []).filter((f) => f.show_in_list);
  const canEdit = level === "edit" || level === "manage" || user?.role === "admin";
  const canManage = level === "manage" || user?.role === "admin";
  const placeLabel = here?.name ?? basket?.name ?? "";
  const isDrive = basket?.system_type === "personal";
  const basketLabel = isDrive ? "My Drive" : basket?.name ?? "";
  const rawTab = (sp.get("tab") ?? "files") as PageTab;
  const tab: PageTab = here ? "files" : rawTab === "settings" && !(canManage && !isDrive) ? "files" : (["files", "access", "activity", "settings"].includes(rawTab) ? rawTab : "files");

  const go = (params: Record<string, string | null>) => {
    const p = new URLSearchParams(sp.toString());
    for (const [k, v] of Object.entries(params)) { if (v == null) p.delete(k); else p.set(k, v); }
    const s = p.toString();
    router.push(`${pathname}${s ? `?${s}` : ""}`);
  };
  const openFile = (id: string) => go({ file: id });
  const toggle = (s: Sel, multi: boolean) => setSel((cur) => {
    const has = cur.some((x) => x.id === s.id);
    if (!multi) return has && cur.length === 1 ? [] : [s];
    return has ? cur.filter((x) => x.id !== s.id) : [...cur, s];
  });
  const selFiles = sel.filter((s) => s.kind === "file").map((s) => s.id);
  const selFolders = sel.filter((s) => s.kind === "folder").map((s) => s.id);
  const allIds: Sel[] = [...kids.map((f) => ({ kind: "folder" as const, id: f.id })), ...(files ?? []).map((f) => ({ kind: "file" as const, id: f.id }))];
  const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"}`;

  const addToDownloads = async (items: DlItem[]) => {
    try {
      const cur = await api<{ item: DlItem }[]>("/me/downloads");
      const have = cur.map((c) => c.item);
      const key = (i: DlItem) => JSON.stringify(i);
      const next = [...have, ...items.filter((i) => !have.some((h) => key(h) === key(i)))];
      await api("/me/downloads", { method: "PUT", json: next });
      nav.setDownloads(next.length);
      toast({ text: `Added ${plural(items.length, "item")} to your download list.` });
    } catch (e) { toast({ text: (e as Error).message, error: true }); }
  };
  const doMove = (fileIds: string[], folderIds: string[]) => setModal(
    <MoveModal baskets={[...(nav.drive ? [nav.drive] : []), ...nav.baskets]} excludeFolders={folderIds}
      count={plural(fileIds.length + folderIds.length, "item")}
      onClose={() => setModal(null)}
      onMove={async (sys, folder, label) => {
        await api("/move", { method: "POST", json: { file_ids: fileIds, folder_ids: folderIds, target_system_id: sys, target_folder_id: folder } });
        toast({ text: `Moved ${plural(fileIds.length + folderIds.length, "item")} to ${label}.` });
        setSel([]); refresh(); nav.reload();
      }} />);
  const doDelete = (fileIds: string[], folderIds: string[]) => setModal(
    <Confirm title={`Delete ${plural(fileIds.length + folderIds.length, "item")}?`} danger action="Delete"
      body={<>Files go to Trash and can be restored for the period your basket keeps them.{folderIds.length > 0 && " Deleting a folder hides everything inside it."}</>}
      onClose={() => setModal(null)}
      onConfirm={async () => {
        for (const id of fileIds) await api(`/files/${id}`, { method: "DELETE" });
        for (const id of folderIds) await api(`/folders/${id}`, { method: "DELETE" });
        toast({ text: `Deleted ${plural(fileIds.length + folderIds.length, "item")}.`, undo: fileIds.length ? async () => {
          for (const id of fileIds) await api(`/files/${id}/restore`, { method: "POST" }).catch(() => {});
          refresh();
        } : undefined });
        setSel([]); if (fileParam && fileIds.includes(fileParam)) go({ file: null }); refresh(); nav.reload();
      }} />);
  const doShare = (rtype: "file" | "folder" | "basket", rid: string, name: string) =>
    setModal(<ShareModal rtype={rtype} rid={rid} name={name} publicLinks={settings?.policy.public_links ?? undefined} onClose={() => { setModal(null); refresh(); }} />);
  const doUpload = (initial?: File[]) => setModal(
    <UploadModal basket={basketId} folder={folderId} folderLabel={placeLabel} settings={settings} initialFiles={initial}
      onClose={() => setModal(null)} onDone={() => { refresh(); nav.reload(); }} />);
  const doNewFolder = () => setModal(<NewFolderModal onClose={() => setModal(null)}
    onSubmit={async ({ name, versioning, encrypted, encryption }) => { await api("/folders", { method: "POST", json: { name, system_id: basketId, parent_id: folderId, versioning, encrypted, encryption } }); toast({ text: `Folder ${name} created` }); refresh(); nav.reload(); }} />);
  const useSt = settings?.use_statuses !== false;
  const doStatus = (f: FileRow) => setModal(<StatusModal fileId={f.id} fileName={f.name} onClose={() => setModal(null)} onDone={refresh} />);

  // Keyboard: Delete removes the selection, Escape closes the panel / clears selection.
  React.useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (tab !== "files" || modal || (e.target as HTMLElement)?.closest("input, textarea, [role=dialog]")) return;
      if (e.key === "Escape") { if (fileParam) go({ file: null }); else setSel([]); }
      if ((e.key === "Delete" || e.key === "Backspace") && sel.length && canEdit) { e.preventDefault(); doDelete(selFiles, selFolders); }
    };
    document.addEventListener("keydown", h);
    return () => document.removeEventListener("keydown", h);
  });

  const missing = (f: FileRow) => {
    if (!settings) return [];
    const m = meta(f.metadata);
    return settings.fields.filter((d) => d.required && (m[d.key] == null || m[d.key] === "")).map((d) => d.label);
  };
  // Header sort: desc → asc → back to newest first.
  const sortBtn = (key: SortKey, label: string, cls?: string) => (
    <th className={cls} aria-sort={sort.key === key ? (sort.dir === "asc" ? "ascending" : "descending") : "none"}>
      <button onClick={() => setSort((s) => s.key !== key ? { key, dir: key === "name" ? "asc" : "desc" }
        : (key === "name" ? s.dir === "asc" : s.dir === "desc") ? { key, dir: s.dir === "asc" ? "desc" : "asc" } : { key: "modified", dir: "desc" })}>
        {label}{sort.key === key ? (sort.dir === "asc" ? <I.arrowUp size={12} /> : <I.arrowDown size={12} />) : <I.sort size={12} />}
      </button>
    </th>
  );
  const cats = (settings?.categories.length ?? 0) > 0;
  const cols = 6 + listFields.length + (cats ? 1 : 0);
  const starred = nav.stars.includes(`basket:${basketId}`);
  const encrypted = !!settings?.policy.encrypt || !!basket?.vault;

  if (err && !basket) return <div className="cx-page"><Note kind="error">{err}</Note></div>;

  const crumbs = (
    <div className="cx-crumbs">
      {!isDrive && <><Link href="/baskets">Baskets</Link><I.chevronRight size={12} /></>}
      {here && basket ? <Link href={`/b/${basketId}`}>{basketLabel}</Link> : <span>{basketLabel || " "}</span>}
      {chain.length > 4 ? (
        <><I.chevronRight size={12} /><span title={chain.slice(0, -2).map((c) => c.name).join(" / ")}>…</span>
          {chain.slice(-2).map((c, i, a) => <React.Fragment key={c.id}><I.chevronRight size={12} />{i === a.length - 1 ? <span>{c.name}</span> : <Link href={`/b/${basketId}?folder=${c.id}`}>{c.name}</Link>}</React.Fragment>)}</>
      ) : chain.map((c, i) => <React.Fragment key={c.id}><I.chevronRight size={12} />{i === chain.length - 1 ? <span>{c.name}</span> : <Link href={`/b/${basketId}?folder=${c.id}`}>{c.name}</Link>}</React.Fragment>)}
    </div>
  );

  const sub = here
    ? (files ? [allKids.length ? plural(allKids.length, "folder") : "", plural(total, "file")].filter(Boolean).join(" · ") : "")
    : [
      !isDrive && basket ? <span key="k" className="cx-mono">{basket.bucket}</span> : null,
      files && !q && !status && !cat ? <span key="f">{allKids.length ? `${plural(allKids.length, "folder")} · ` : ""}{plural(total, "file")}</span> : null,
      usage ? <span key="s">{bytes(usage.used)}{usage.cap > 0 ? ` of ${bytes(usage.cap)}` : ""}</span> : null,
      encrypted ? <Tag key="e" color="purple">Encrypted</Tag> : null,
      basket?.vault ? <Tag key="v" color="red">Vault</Tag> : null,
      basket?.mount_path ? <Tag key="m" color="gray">Mounted</Tag> : null,
    ].filter(Boolean).flatMap((x, i) => (i ? [<span key={`d${i}`} aria-hidden>·</span>, x] : [x]));

  return (
    <div className="cx-page">
      {crumbs}
      <div className="cx-ph">
        <div className="cx-ph-main">
          {!here && basket && <span className="cx-bicon" style={{ background: basketColor(basket.tone), width: 48, height: 48 }}>{isDrive ? <I.user size={20} /> : <I.folder size={20} />}</span>}
          <div style={{ minWidth: 0 }}>
            <h1 className="cx-title">{here?.name ?? (basketLabel || " ")}
              {!here && basket && !isDrive && (
                <button className="cx-star" aria-pressed={starred} aria-label={starred ? "Remove star" : "Star"}
                  onClick={() => nav.toggleStar("basket", basketId)}>{starred ? <I.starFilled size={20} /> : <I.star size={20} />}</button>
              )}
            </h1>
            <div className="cx-sub cx-num">{sub}</div>
          </div>
        </div>
        {tab === "files" && (
          <div className="cx-acts">
            {canManage && <Btn kind="ghost" md className="cx-hide-sm" icon={<I.share />} onClick={() => doShare(here ? "folder" : "basket", here?.id ?? basketId, here?.name ?? basket?.name ?? "")}>Access</Btn>}
            {canEdit && <Btn kind="tertiary" md className="cx-hide-sm" icon={<I.add />} onClick={doNewFolder}>New folder</Btn>}
            {canEdit && <Btn kind="primary" md icon={<I.upload />} onClick={() => doUpload()}>Upload</Btn>}
          </div>
        )}
      </div>

      {!here && basket && (
        <div className="cx-tabs cx-ptabs" role="tablist">
          {([["files", "Files"], ["access", "Access"], ["activity", "Activity"], ...(canManage && !isDrive ? [["settings", "Settings"]] : [])] as [PageTab, string][]).map(([id, label]) => (
            <button key={id} role="tab" className="cx-tab" aria-selected={tab === id} onClick={() => go({ tab: id === "files" ? null : id, file: null })}>
              {label}{id === "files" && files && !q && !status && !cat ? <small>{total}</small> : null}
            </button>
          ))}
        </div>
      )}

      {tab === "access" && basket && <BasketAccess basket={basket} canManage={canManage} onManage={() => doShare("basket", basketId, basket.name)} />}
      {tab === "activity" && basket && <BasketActivity basketId={basketId} />}
      {tab === "settings" && basket && <BasketSettings id={basketId} embedded />}

      {tab === "files" && (
      <div className={`cx-explorer${folders?.length ? " cx-has-tree" : ""}`}>
      {folders?.length && basket ? <FolderTree basketId={basketId} rootLabel={basketLabel} folders={folders} current={folderId} /> : null}
      <div className={`cx-work${fileParam ? " cx-has-panel" : ""}`}>
        <div className={`cx-list-wrap${dropping ? " cx-dropping" : ""}`}
          onDragOver={(e) => { if (canEdit && e.dataTransfer.types.includes("Files")) { e.preventDefault(); setDropping(true); } }}
          onDragLeave={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setDropping(false); }}
          onDrop={(e) => { if (!canEdit) return; e.preventDefault(); setDropping(false); if (e.dataTransfer.files.length) doUpload(Array.from(e.dataTransfer.files)); }}>
          {sel.length > 0 ? (
            <div className="cx-selbar" role="toolbar" aria-label="Selection actions">
              <span className="cx-selcount cx-num">{sel.length} selected</span>
              <span className="cx-spacer" />
              {selFiles.length === 1 && selFolders.length === 0 && <a className="cx-btn" href={`${BASE_PATH}/api/files/${selFiles[0]}/download`}><I.download /><span className="cx-hide-sm">Download</span></a>}
              <Btn onClick={() => addToDownloads([...selFiles.map((id) => ({ kind: "file" as const, id })), ...selFolders.map((id) => ({ kind: "folder" as const, id }))])}><I.add /><span className="cx-hide-sm">Add to download list</span></Btn>
              {canEdit && <Btn onClick={() => doMove(selFiles, selFolders)}><I.move /><span className="cx-hide-sm">Move</span></Btn>}
              {sel.length === 1 && <Btn onClick={() => {
                const s = sel[0];
                const name = s.kind === "file" ? files?.find((f) => f.id === s.id)?.name : kids.find((f) => f.id === s.id)?.name;
                doShare(s.kind, s.id, name ?? "");
              }}><I.share /><span className="cx-hide-sm">Share</span></Btn>}
              {useSt && selFiles.length === 1 && selFolders.length === 0 && <Btn onClick={() => { const f = files?.find((x) => x.id === selFiles[0]); if (f) doStatus(f); }}><I.review /><span className="cx-hide-sm">Change status</span></Btn>}
              {canEdit && <Btn onClick={() => doDelete(selFiles, selFolders)}><I.trash /><span className="cx-hide-sm">Delete</span></Btn>}
              <Btn className="cx-sel-cancel" onClick={() => setSel([])}>Cancel</Btn>
            </div>
          ) : (
            <div className="cx-toolbar" role="toolbar" aria-label="Basket actions">
              <div className="cx-grow cx-search-wrap">
                <I.search />
                <input className="cx-search" placeholder={`Search in ${placeLabel || "this basket"}`} value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search in this folder" />
              </div>
              {cats && (
                <Dropdown className="cx-hide-sm" value={cat} placeholder="Category" renderValue={cat ? settings?.categories.find((c) => c.code === cat)?.name : undefined}
                  options={[{ value: "", label: "All categories" }, ...(settings?.categories ?? []).map((c) => ({ value: c.code, label: c.name }))]} onChange={setCat} />
              )}
              {useSt && <Dropdown className="cx-hide-sm" value={status} placeholder="Status" renderValue={status ? settings?.statuses.find((s) => s.code === status)?.label : undefined}
                options={[{ value: "", label: "All statuses" }, ...(settings?.statuses ?? []).map((s) => ({ value: s.code, label: s.label }))]} onChange={setStatus} />}
              {canEdit && <Btn kind="ghost" className="cx-hide-sm" iconOnly aria-label="Upload" title="Upload" icon={<I.upload />} onClick={() => doUpload()} />}
            </div>
          )}

          <div className="cx-table-scroll">
            <table className="cx-table">
              <thead>
                <tr>
                  <th className="cx-check"><input type="checkbox" aria-label="Select all"
                    checked={allIds.length > 0 && sel.length === allIds.length} onChange={(e) => setSel(e.target.checked ? allIds : [])} /></th>
                  {sortBtn("name", "Name")}
                  {cats ? <th className="cx-hide-sm cx-hide-md">Category</th> : null}
                  {listFields.map((f) => <th key={f.key} className="cx-hide-sm">{f.label}</th>)}
                  {sortBtn("owner", "Owner", "cx-hide-sm")}
                  {useSt && sortBtn("status", "Status")}
                  {sortBtn("size", "Size", "cx-right cx-hide-sm")}
                  <th style={{ width: 48 }} />
                </tr>
              </thead>
              <tbody>
                {(files == null || folders == null) && <SkeletonRows cols={cols} />}
                {files && folders && kids.map((f) => {
                  const s = { kind: "folder" as const, id: f.id };
                  const on = sel.some((x) => x.id === f.id);
                  return (
                    <tr key={f.id} aria-selected={on} onDoubleClick={() => go({ folder: f.id, file: null })}>
                      <td className="cx-check"><input type="checkbox" aria-label={`Select ${f.name}`} checked={on} onChange={() => toggle(s, true)} /></td>
                      <td className="cx-name"><Link className="cx-fname" href={`/b/${basketId}?folder=${f.id}`}><I.folder size={20} className="cx-folder-ic" />
                        <span><span className="cx-fname-t">{f.name}</span><span className="cx-fname-meta cx-num">{when(f.created_at, false)}{!f.inherit ? " · own access" : ""}</span></span></Link></td>
                      {cats ? <td className="cx-hide-sm cx-hide-md" /> : null}
                      {listFields.map((d) => <td key={d.key} className="cx-hide-sm" />)}
                      <td className="cx-hide-sm" />
                      <td />
                      <td className="cx-right cx-hide-sm" />
                      <td className="cx-ovf"><OverflowMenu items={[
                        { label: "Open", icon: <I.folderOpen />, onClick: () => go({ folder: f.id, file: null }) },
                        ...(canEdit ? [{ label: "Rename", icon: <I.edit />, onClick: () => setModal(<NameModal title="Rename folder" action="Rename" initial={f.name} onClose={() => setModal(null)}
                          onSubmit={async (name) => { await api(`/folders/${f.id}`, { method: "PATCH", json: { name } }); toast({ text: "Folder renamed." }); refresh(); nav.reload(); }} />) },
                          { label: "Move", icon: <I.move />, onClick: () => doMove([], [f.id]) }] : []),
                        { label: "Details", icon: <I.doc />, onClick: () => setModal(<FolderDetailsModal folderId={f.id} onClose={() => setModal(null)} />) },
                        { label: "Download folder (zip)", icon: <I.download />, onClick: () => { window.location.href = `${BASE_PATH}/api/folders/${f.id}/zip`; } },
                        { label: "Share", icon: <I.share />, onClick: () => doShare("folder", f.id, f.name) },
                        { label: "Add to download list", icon: <I.add />, onClick: () => addToDownloads([{ kind: "folder", id: f.id }]) },
                        ...(canEdit ? ["sep" as const, { label: "Delete", icon: <I.trash />, danger: true, onClick: () => doDelete([], [f.id]) }] : []),
                      ]} /></td>
                    </tr>
                  );
                })}
                {files && folders && files.map((f) => {
                  const s = { kind: "file" as const, id: f.id };
                  const on = sel.some((x) => x.id === f.id) || fileParam === f.id;
                  const miss = missing(f);
                  const m = meta(f.metadata);
                  const locks = !!settings?.statuses.find((st) => st.code === f.status)?.locks_content;
                  return (
                    <React.Fragment key={f.id}>
                      <tr aria-selected={on} onDoubleClick={() => router.push(`/view/${f.id}`)}>
                        <td className="cx-check"><input type="checkbox" aria-label={`Select ${f.name}`} checked={sel.some((x) => x.id === f.id)} onChange={() => toggle(s, true)} /></td>
                        <td className="cx-name"><button className="cx-fname" onClick={() => openFile(f.id)}><FileTile name={f.name} />
                          <span><span className="cx-fname-t">{f.name}</span>
                            <span className="cx-fname-meta cx-num">{f.version > 1 ? `Version ${f.version} · ` : ""}{when(f.modified_at)}{f.encrypted ? " · Encrypted" : ""}</span></span></button></td>
                        {cats ? <td className="cx-hide-sm cx-hide-md">{settings?.categories.find((c) => c.code === f.category_code)?.name ?? ""}</td> : null}
                        {listFields.map((d) => <td key={d.key} className="cx-hide-sm">{m[d.key] == null ? "" : String(m[d.key])}</td>)}
                        <td className="cx-hide-sm"><Who name={f.owner} /></td>
                        {useSt && <td><StatusTag code={f.status} statuses={settings?.statuses} /></td>}
                        <td className="cx-right cx-num cx-hide-sm">{bytes(f.size_bytes)}</td>
                        <td className="cx-ovf"><OverflowMenu items={[
                          { label: "Open", icon: <I.doc />, onClick: () => router.push(`/view/${f.id}`) },
                          { label: "Details", icon: <I.rules />, onClick: () => openFile(f.id) },
                          { label: "Download", icon: <I.download />, onClick: () => { window.location.href = `${BASE_PATH}/api/files/${f.id}/download`; } },
                          { label: "Add to download list", icon: <I.add />, onClick: () => addToDownloads([{ kind: "file", id: f.id }]) },
                          ...(canEdit && !locks ? [{ label: "Rename", icon: <I.edit />, onClick: () => setModal(<NameModal title="Rename file" action="Rename" initial={f.name} onClose={() => setModal(null)}
                            onSubmit={async (name) => { await api(`/files/${f.id}`, { method: "PATCH", json: { name } }); toast({ text: "File renamed." }); refresh(); }} />) }] : []),
                          ...(canEdit ? [{ label: "Move", icon: <I.move />, onClick: () => doMove([f.id], []) }] : []),
                          { label: "Share", icon: <I.share />, onClick: () => doShare("file", f.id, f.name) },
                          ...(useSt ? [{ label: "Change status", icon: <I.review />, onClick: () => doStatus(f) }] : []),
                          ...(canManage && !f.encrypted ? [{ label: "Encrypt file", icon: <I.lock />, onClick: () => setModal(<Confirm title={`Encrypt ${f.name}?`} action="Encrypt" body="The file and its earlier versions are stored encrypted. People with access open it as before."
                            onClose={() => setModal(null)} onConfirm={async () => { await api(`/files/${f.id}/encrypt`, { method: "POST" }); toast({ text: `${f.name} is encrypted.` }); refresh(); }} />) }] : []),
                          ...(canEdit ? ["sep" as const, { label: "Delete", icon: <I.trash />, danger: true, onClick: () => doDelete([f.id], []) }] : []),
                        ]} /></td>
                      </tr>
                      {miss.length > 0 && (
                        <tr className="cx-gaprow"><td colSpan={cols}>Missing: {miss.join(", ")}{canEdit && !locks && <> · <button className="cx-filelink" style={{ color: "inherit", textDecoration: "underline" }} onClick={() => openFile(f.id)}>Fill in</button></>}</td></tr>
                      )}
                    </React.Fragment>
                  );
                })}
              </tbody>
            </table>
            {files && folders && kids.length === 0 && files.length === 0 && (
              <Empty action={(qDeb || status || cat) ? <Btn kind="ghost" onClick={() => { setQ(""); setStatus(""); setCat(""); }}>Clear filters</Btn> : undefined}>
                {qDeb || status || cat ? "No files match these filters" : `${placeLabel || "This folder"} has no files yet`}
              </Empty>
            )}
          </div>
          {files && total > 0 && <Pager page={page} size={size} total={total} onPage={(p) => { setPage(p); setSel([]); }} onSize={setSize} />}
          {err && basket && <Note kind="error">{err} <button className="cx-filelink" style={{ textDecoration: "underline" }} onClick={() => { setErr(null); refresh(); }}>Try again</button></Note>}
        </div>
        {fileParam && <>
          <div className="cx-panel-scrim" onClick={() => go({ file: null })} />
          <FilePanel fileId={fileParam} settingsFor={loadSettings} refreshKey={tick} onClose={() => go({ file: null })} onChanged={refresh}
            basketName={basketLabel} folderPath={chain.map((c) => c.name).join(" / ")}
            onShare={(f) => doShare("file", f.id, f.name)}
            onStatus={doStatus} />
        </>}
      </div>
      </div>
      )}
      {modal}
    </div>
  );
}
