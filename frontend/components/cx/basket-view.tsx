"use client";

import * as React from "react";
import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";

import { useAuth } from "@/lib/auth-context";
import {
  api, bytes, ext, meta, when, BASE_PATH,
  type Basket, type DlItem, type FileRow, type Folder, type Settings,
} from "@/lib/fh";
import { FilePanel } from "./file-panel";
import { FolderDetailsModal, MoveModal, NameModal, NewFolderModal, ShareModal, StatusModal, UploadModal } from "./modals";
import { FolderTree } from "./folder-tree";
import { useNav } from "./shell";
import { Btn, Confirm, Dropdown, I, Note, OverflowMenu, SkeletonRows, StatusTag, Tag, useToast, When } from "./ui";

const settingsCache = new Map<string, Promise<Settings | null>>();
export function loadSettings(basket: string, fresh = false): Promise<Settings | null> {
  if (fresh || !settingsCache.has(basket)) settingsCache.set(basket, api<Settings>(`/baskets/${basket}/settings`).catch(() => null));
  return settingsCache.get(basket)!;
}

type Sel = { kind: "file" | "folder"; id: string };
type SortKey = "name" | "modified" | "size" | "status";

/// FH-01 Basket view: folders + files of one folder, toolbar, selection bar,
/// side panel. Columns come from the basket's settings, not from code.
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
  const [level, setLevel] = React.useState<string>("none");
  const [err, setErr] = React.useState<string | null>(null);
  const [q, setQ] = React.useState("");
  const [qDeb, setQDeb] = React.useState("");
  const [cat, setCat] = React.useState("");
  const [status, setStatus] = React.useState("");
  const [sort, setSort] = React.useState<{ key: SortKey; dir: "asc" | "desc" }>({ key: "modified", dir: "desc" });
  const [sel, setSel] = React.useState<Sel[]>([]);
  const [tick, setTick] = React.useState(0);
  const [modal, setModal] = React.useState<React.ReactNode>(null);
  const [dropping, setDropping] = React.useState(false);
  const refresh = React.useCallback(() => setTick((t) => t + 1), []);

  React.useEffect(() => { const t = setTimeout(() => setQDeb(q), 250); return () => clearTimeout(t); }, [q]);
  React.useEffect(() => { setSel([]); }, [basketId, folderId]);

  // Basket, settings, every folder of the basket (breadcrumb + children), access level.
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

  React.useEffect(() => {
    let off = false;
    setFiles(null);
    const p = new URLSearchParams({ system_id: basketId, folder_id: folderId ?? "null", sort: sort.key === "modified" ? "modified" : sort.key, dir: sort.dir, limit: "500" });
    if (qDeb) p.set("q", qDeb);
    if (status) p.set("status", status);
    if (cat) p.set("category", cat);
    fetch(`${BASE_PATH}/api/files?${p}`, { cache: "no-store" }).then(async (r) => {
      if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error ?? `Couldn't load files (${r.status}).`);
      const rows = (await r.json()) as FileRow[];
      if (!off) { setFiles(rows); setTotal(Number(r.headers.get("x-total-count") ?? rows.length)); }
    }).catch((e) => { if (!off) { setFiles([]); setErr(e.message); } });
    return () => { off = true; };
  }, [basketId, folderId, qDeb, status, cat, sort, tick]);

  const chain = React.useMemo(() => {
    const out: Folder[] = [];
    let cur = folders?.find((f) => f.id === folderId) ?? null;
    let guard = 0;
    while (cur && guard++ < 64) { out.unshift(cur); cur = folders?.find((f) => f.id === cur!.parent_id) ?? null; }
    return out;
  }, [folders, folderId]);
  const here = chain[chain.length - 1];
  const kids = React.useMemo(() => {
    const list = (folders ?? []).filter((f) => (f.parent_id ?? null) === (folderId ?? null));
    const fq = qDeb.toLowerCase();
    return (fq ? list.filter((f) => f.name.toLowerCase().includes(fq)) : list).sort((a, b) => a.name.localeCompare(b.name));
  }, [folders, folderId, qDeb]);
  const listFields = (settings?.fields ?? []).filter((f) => f.show_in_list);
  const canEdit = level === "edit" || level === "manage" || user?.role === "admin";
  const canManage = level === "manage" || user?.role === "admin";
  const placeLabel = here?.name ?? basket?.name ?? "";
  const isDrive = basket?.system_type === "personal";

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

  const addToDownloads = async (items: DlItem[]) => {
    try {
      const cur = await api<{ item: DlItem }[]>("/me/downloads");
      const have = cur.map((c) => c.item);
      const key = (i: DlItem) => JSON.stringify(i);
      const next = [...have, ...items.filter((i) => !have.some((h) => key(h) === key(i)))];
      await api("/me/downloads", { method: "PUT", json: next });
      nav.setDownloads(next.length);
      toast({ text: `Added ${items.length} item${items.length > 1 ? "s" : ""} to your download list.` });
    } catch (e) { toast({ text: (e as Error).message, error: true }); }
  };
  const doMove = (fileIds: string[], folderIds: string[]) => setModal(
    <MoveModal baskets={[...(nav.drive ? [nav.drive] : []), ...nav.baskets]} excludeFolders={folderIds}
      count={`${fileIds.length + folderIds.length} item${fileIds.length + folderIds.length > 1 ? "s" : ""}`}
      onClose={() => setModal(null)}
      onMove={async (sys, folder, label) => {
        await api("/move", { method: "POST", json: { file_ids: fileIds, folder_ids: folderIds, target_system_id: sys, target_folder_id: folder } });
        toast({ text: `Moved ${fileIds.length + folderIds.length} item${fileIds.length + folderIds.length > 1 ? "s" : ""} to ${label}.` });
        setSel([]); refresh(); nav.reload();
      }} />);
  const doDelete = (fileIds: string[], folderIds: string[]) => setModal(
    <Confirm title={`Delete ${fileIds.length + folderIds.length} item${fileIds.length + folderIds.length > 1 ? "s" : ""}?`} danger action="Delete"
      body={<>Files go to Trash and can be restored for the period your basket keeps them.{folderIds.length > 0 && " Deleting a folder hides everything inside it."}</>}
      onClose={() => setModal(null)}
      onConfirm={async () => {
        for (const id of fileIds) await api(`/files/${id}`, { method: "DELETE" });
        for (const id of folderIds) await api(`/folders/${id}`, { method: "DELETE" });
        toast({ text: `Deleted ${fileIds.length + folderIds.length} item${fileIds.length + folderIds.length > 1 ? "s" : ""}.`, undo: fileIds.length ? async () => {
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

  // Keyboard: Delete removes the selection, Escape closes the panel / clears selection.
  React.useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (modal || (e.target as HTMLElement)?.closest("input, textarea, [role=dialog]")) return;
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
  const sortBtn = (key: SortKey, label: string, right?: boolean, cls?: string) => (
    <th className={[right ? "cx-right" : "", cls ?? ""].join(" ").trim() || undefined} aria-sort={sort.key === key ? (sort.dir === "asc" ? "ascending" : "descending") : "none"}>
      <button onClick={() => setSort((s) => ({ key, dir: s.key === key && s.dir === "desc" ? "asc" : "desc" }))}>
        {label}{sort.key === key ? (sort.dir === "asc" ? <I.arrowUp size={12} /> : <I.arrowDown size={12} />) : null}
      </button>
    </th>
  );
  const cols = 6 + listFields.length;

  if (err && !basket) return <div className="cx-page"><Note kind="error">{err}</Note></div>;

  return (
    <div className="cx-page">
      <div className="cx-crumbs">
        {here && basket && <Link href={`/b/${basketId}`}>{isDrive ? "My Drive" : basket.name}</Link>}
        {chain.slice(0, -1).length > 3 ? (
          <><span>/</span><span title={chain.slice(0, -1).map((c) => c.name).join(" / ")}>…</span>
            {chain.slice(-2, -1).map((c) => <React.Fragment key={c.id}><span>/</span><Link href={`/b/${basketId}?folder=${c.id}`}>{c.name}</Link></React.Fragment>)}</>
        ) : chain.slice(0, -1).map((c) => <React.Fragment key={c.id}><span>/</span><Link href={`/b/${basketId}?folder=${c.id}`}>{c.name}</Link></React.Fragment>)}
      </div>
      <div className="cx-title-row">
        <h1 className="cx-title">{here?.name ?? (isDrive ? "My Drive" : basket?.name) ?? " "}</h1>
        {basket?.vault && <Tag color="blue"><I.lock size={10} /> Vault</Tag>}
        {!here && basket && !isDrive && (
          <button className="cx-star" aria-pressed={nav.stars.includes(`basket:${basketId}`)}
            aria-label={nav.stars.includes(`basket:${basketId}`) ? "Remove star" : "Star to show in the side menu"}
            title={nav.stars.includes(`basket:${basketId}`) ? "Starred — shown in the side menu" : "Star to show in the side menu"}
            onClick={() => nav.toggleStar("basket", basketId)}>{nav.stars.includes(`basket:${basketId}`) ? <I.starFilled /> : <I.star />}</button>
        )}
        <span className="cx-sub cx-num">{files ? `${kids.length ? `${kids.length} folder${kids.length > 1 ? "s" : ""} · ` : ""}${total} file${total === 1 ? "" : "s"}` : ""}</span>
        <span className="cx-spacer" />
        {canManage && <Btn kind="ghost" onClick={() => doShare(here ? "folder" : "basket", here?.id ?? basketId, here?.name ?? basket?.name ?? "")}>Access</Btn>}
        {canManage && !here && !isDrive && <Link className="cx-btn cx-btn-ghost" href={`/admin/baskets/${basketId}`}>Settings</Link>}
      </div>

      {/* The folders pane appears only when the basket has folders (like a file explorer). */}
      <div className={`cx-explorer${folders?.length ? " cx-has-tree" : ""}`}>
      {folders?.length && basket ? <FolderTree basketId={basketId} rootLabel={isDrive ? "My Drive" : basket.name} folders={folders} current={folderId} /> : null}
      <div className={`cx-work${fileParam ? " cx-has-panel" : ""}`}>
        <div className={`cx-list-wrap${dropping ? " cx-dropping" : ""}`}
          onDragOver={(e) => { if (canEdit && e.dataTransfer.types.includes("Files")) { e.preventDefault(); setDropping(true); } }}
          onDragLeave={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setDropping(false); }}
          onDrop={(e) => { if (!canEdit) return; e.preventDefault(); setDropping(false); if (e.dataTransfer.files.length) doUpload(Array.from(e.dataTransfer.files)); }}>
          {sel.length > 0 ? (
            <div className="cx-selbar" role="toolbar" aria-label="Selection actions">
              <span className="cx-selcount cx-num">{sel.length} selected</span>
              {selFiles.length === 1 && selFolders.length === 0 && <a className="cx-btn" href={`${BASE_PATH}/api/files/${selFiles[0]}/download`}>Download</a>}
              <Btn onClick={() => addToDownloads([...selFiles.map((id) => ({ kind: "file" as const, id })), ...selFolders.map((id) => ({ kind: "folder" as const, id }))])}>Add to download list</Btn>
              {canEdit && <Btn onClick={() => doMove(selFiles, selFolders)}>Move</Btn>}
              {sel.length === 1 && <Btn onClick={() => {
                const s = sel[0];
                const name = s.kind === "file" ? files?.find((f) => f.id === s.id)?.name : kids.find((f) => f.id === s.id)?.name;
                doShare(s.kind, s.id, name ?? "");
              }}>Share</Btn>}
              {selFiles.length === 1 && selFolders.length === 0 && <Btn onClick={() => {
                const f = files?.find((x) => x.id === selFiles[0]); if (f) setModal(<StatusModal fileId={f.id} fileName={f.name} onClose={() => setModal(null)} onDone={refresh} />);
              }}>Change status</Btn>}
              {canEdit && <Btn onClick={() => doDelete(selFiles, selFolders)}>Delete</Btn>}
              <span className="cx-spacer" />
              <Btn onClick={() => setSel([])}>Cancel</Btn>
            </div>
          ) : (
            <div className="cx-toolbar" role="toolbar" aria-label="Basket actions">
              <div className="cx-grow cx-search-wrap">
                <I.search />
                <input className="cx-search" placeholder={`Search in ${placeLabel || "this basket"}`} value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search in this folder" />
              </div>
              {(settings?.categories.length ?? 0) > 0 && (
                <Dropdown className="cx-hide-sm" value={cat} placeholder="Category" renderValue={cat ? settings?.categories.find((c) => c.code === cat)?.name : undefined}
                  options={[{ value: "", label: "All categories" }, ...(settings?.categories ?? []).map((c) => ({ value: c.code, label: c.name }))]} onChange={setCat} />
              )}
              <Dropdown className="cx-hide-sm" value={status} placeholder="Status" renderValue={status ? settings?.statuses.find((s) => s.code === status)?.label : undefined}
                options={[{ value: "", label: "All statuses" }, ...(settings?.statuses ?? []).map((s) => ({ value: s.code, label: s.label }))]} onChange={setStatus} />
              {canEdit && <Btn kind="ghost" className="cx-hide-sm" onClick={() => setModal(<NewFolderModal onClose={() => setModal(null)}
                onSubmit={async ({ name, versioning, encrypted }) => { await api("/folders", { method: "POST", json: { name, system_id: basketId, parent_id: folderId, versioning, encrypted } }); toast({ text: `สร้างโฟลเดอร์ ${name} แล้ว` }); refresh(); nav.reload(); }} />)}>New folder</Btn>}
              {canEdit && <Btn kind="primary" icon={<I.upload />} onClick={() => doUpload()}>Upload</Btn>}
            </div>
          )}

          <div className="cx-table-scroll">
            <table className="cx-table">
              <thead>
                <tr>
                  <th className="cx-check"><input type="checkbox" aria-label="Select all"
                    checked={allIds.length > 0 && sel.length === allIds.length} onChange={(e) => setSel(e.target.checked ? allIds : [])} /></th>
                  {sortBtn("name", "Name")}
                  {settings?.categories.length ? <th className="cx-hide-sm cx-hide-md">Category</th> : null}
                  {sortBtn("status", "Status")}
                  {listFields.map((f) => <th key={f.key} className="cx-hide-sm">{f.label}</th>)}
                  {sortBtn("modified", "Modified", false, "cx-hide-sm")}
                  <th className="cx-right cx-hide-sm">Size</th>
                  <th style={{ width: 32 }} />
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
                      <td className="cx-name"><Link className="cx-filelink" href={`/b/${basketId}?folder=${f.id}`}><I.folder /><span>{f.name}</span></Link>{!f.inherit && <span className="cx-helper"> · own access</span>}</td>
                      {settings?.categories.length ? <td className="cx-hide-sm cx-hide-md" /> : null}
                      <td />
                      {listFields.map((d) => <td key={d.key} className="cx-hide-sm" />)}
                      <td className="cx-num cx-hide-sm">{when(f.created_at, false)}</td>
                      <td className="cx-right cx-hide-sm" />
                      <td><OverflowMenu items={[
                        { label: "Open", onClick: () => go({ folder: f.id, file: null }) },
                        ...(canEdit ? [{ label: "Rename", onClick: () => setModal(<NameModal title="Rename folder" action="Rename" initial={f.name} onClose={() => setModal(null)}
                          onSubmit={async (name) => { await api(`/folders/${f.id}`, { method: "PATCH", json: { name } }); toast({ text: "Folder renamed." }); refresh(); nav.reload(); }} />) },
                          { label: "Move", onClick: () => doMove([], [f.id]) }] : []),
                        { label: "รายละเอียด", onClick: () => setModal(<FolderDetailsModal folderId={f.id} onClose={() => setModal(null)} />) },
                        { label: "ดาวน์โหลดทั้งโฟลเดอร์ (zip)", onClick: () => { window.location.href = `${BASE_PATH}/api/folders/${f.id}/zip`; } },
                        { label: "Share", onClick: () => doShare("folder", f.id, f.name) },
                        { label: "Add to download list", onClick: () => addToDownloads([{ kind: "folder", id: f.id }]) },
                        ...(canEdit ? ["sep" as const, { label: "Delete", danger: true, onClick: () => doDelete([], [f.id]) }] : []),
                      ]} /></td>
                    </tr>
                  );
                })}
                {files && folders && files.map((f) => {
                  const s = { kind: "file" as const, id: f.id };
                  const on = sel.some((x) => x.id === f.id) || fileParam === f.id;
                  const miss = missing(f);
                  const m = meta(f.metadata);
                  return (
                    <React.Fragment key={f.id}>
                      <tr aria-selected={on} onDoubleClick={() => router.push(`/view/${f.id}`)}>
                        <td className="cx-check"><input type="checkbox" aria-label={`Select ${f.name}`} checked={sel.some((x) => x.id === f.id)} onChange={() => toggle(s, true)} /></td>
                        <td className="cx-name"><button className="cx-filelink" onClick={() => openFile(f.id)}><span className="cx-ftype">{ext(f.name)}</span><span>{f.name}</span></button>
                          {f.encrypted && <span title="Encrypted" style={{ marginLeft: 6, color: "var(--cx-text-helper)" }}><I.lock size={12} /></span>}</td>
                        {settings?.categories.length ? <td className="cx-hide-sm cx-hide-md">{settings.categories.find((c) => c.code === f.category_code)?.name ?? ""}</td> : null}
                        <td><StatusTag code={f.status} statuses={settings?.statuses} /></td>
                        {listFields.map((d) => <td key={d.key} className="cx-hide-sm">{m[d.key] == null ? "" : String(m[d.key])}</td>)}
                        <td className="cx-hide-sm"><When iso={f.modified_at} /></td>
                        <td className="cx-right cx-num cx-hide-sm">{bytes(f.size_bytes)}</td>
                        <td><OverflowMenu items={[
                          { label: "Open", onClick: () => router.push(`/view/${f.id}`) },
                          { label: "Details", onClick: () => openFile(f.id) },
                          { label: "Download", onClick: () => { window.location.href = `${BASE_PATH}/api/files/${f.id}/download`; } },
                          { label: "Add to download list", onClick: () => addToDownloads([{ kind: "file", id: f.id }]) },
                          ...(canEdit ? [{ label: "Move", onClick: () => doMove([f.id], []) }] : []),
                          { label: "Share", onClick: () => doShare("file", f.id, f.name) },
                          { label: "Change status", onClick: () => setModal(<StatusModal fileId={f.id} fileName={f.name} onClose={() => setModal(null)} onDone={refresh} />) },
                          ...(canManage && !f.encrypted ? [{ label: "Encrypt file", onClick: () => setModal(<Confirm title={`Encrypt ${f.name}?`} action="Encrypt" body="The file and its earlier versions are stored encrypted. People with access open it as before."
                            onClose={() => setModal(null)} onConfirm={async () => { await api(`/files/${f.id}/encrypt`, { method: "POST" }); toast({ text: `${f.name} is encrypted.` }); refresh(); }} />) }] : []),
                          ...(canEdit ? ["sep" as const, { label: "Delete", danger: true, onClick: () => doDelete([f.id], []) }] : []),
                        ]} /></td>
                      </tr>
                      {miss.length > 0 && (
                        <tr className="cx-gaprow"><td colSpan={cols}>Missing: {miss.join(", ")}{canEdit && !settings?.statuses.find((st) => st.code === f.status)?.locks_content && <> · <button className="cx-filelink" style={{ color: "inherit", textDecoration: "underline" }} onClick={() => openFile(f.id)}>Fill in</button></>}</td></tr>
                      )}
                    </React.Fragment>
                  );
                })}
              </tbody>
            </table>
            {files && folders && kids.length === 0 && files.length === 0 && (
              <div className="cx-state">
                <strong>{qDeb || status || cat ? "No files match these filters." : "No files in this folder."}</strong>
                {(qDeb || status || cat) && <Btn kind="ghost" onClick={() => { setQ(""); setStatus(""); setCat(""); }}>Clear filters</Btn>}
              </div>
            )}
          </div>
          {err && basket && <Note kind="error">{err} <button className="cx-filelink" style={{ textDecoration: "underline" }} onClick={() => { setErr(null); refresh(); }}>Try again</button></Note>}
        </div>
        {fileParam && <>
          <div className="cx-panel-scrim" onClick={() => go({ file: null })} />
          <FilePanel fileId={fileParam} settingsFor={loadSettings} refreshKey={tick} onClose={() => go({ file: null })} onChanged={refresh}
            basketName={isDrive ? "My Drive" : basket?.name} folderPath={chain.map((c) => c.name).join(" › ")}
            onShare={(f) => doShare("file", f.id, f.name)}
            onStatus={(f) => setModal(<StatusModal fileId={f.id} fileName={f.name} onClose={() => setModal(null)} onDone={refresh} />)} />
        </>}
      </div>
      </div>
      {modal}
    </div>
  );
}
