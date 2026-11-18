"use client";

import * as React from "react";
import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";

import { useAuth } from "@/lib/auth-context";
import {
  api, bytes, ext, meta, when, BASE_PATH,
  type Basket, type DlItem, type FieldOption, type Folder, type Rule, type Settings, type VBasket, type VTree,
} from "@/lib/fh";
import { loadSettings } from "./basket-view";
import { describeRules, FilePanel } from "./file-panel";
import { ShareModal, StatusModal, UploadModal } from "./modals";
import { useNav } from "./shell";
import { Btn, Check, Confirm, Dropdown, I, Note, OverflowMenu, SkeletonRows, StatusTag, Tag, TextField, useToast, When } from "./ui";

const NONE = "__none__";

/// FH-02 Virtual Basket view: groups from Group by become folders; files show
/// where they really live (Basket column).
export function VirtualView({ id }: { id: string }) {
  const router = useRouter();
  const pathname = usePathname();
  const sp = useSearchParams();
  const path = (sp.get("path") ?? "").split("/").filter(Boolean).map(decodeURIComponent);
  const fileParam = sp.get("file");
  const nav = useNav();
  const toast = useToast();
  const [vb, setVb] = React.useState<VBasket | null>(null);
  const [tree, setTree] = React.useState<VTree | null>(null);
  const [statuses, setStatuses] = React.useState<Settings["statuses"]>([]);
  const [err, setErr] = React.useState<string | null>(null);
  const [q, setQ] = React.useState("");
  const [qDeb, setQDeb] = React.useState("");
  const [tick, setTick] = React.useState(0);
  const [modal, setModal] = React.useState<React.ReactNode>(null);
  const [dropSettings, setDropSettings] = React.useState<Settings | null>(null);
  const refresh = () => setTick((t) => t + 1);
  React.useEffect(() => { const t = setTimeout(() => setQDeb(q), 250); return () => clearTimeout(t); }, [q]);

  React.useEffect(() => {
    let off = false;
    setErr(null);
    api<VBasket>(`/vbaskets/${id}`).then(async (v) => {
      if (off) return;
      setVb(v);
      const ss = await Promise.all((v.all_sources ? nav.baskets.map((b) => b.id) : v.sources).map((s) => loadSettings(s)));
      const seen = new Map<string, Settings["statuses"][number]>();
      for (const s of ss) for (const st of s?.statuses ?? []) if (!seen.has(st.code)) seen.set(st.code, st);
      if (!off) setStatuses([...seen.values()]);
      if (v.drop_target_system_id) setDropSettings(await loadSettings(v.drop_target_system_id));
    }).catch((e) => { if (!off) setErr(e.message); });
    return () => { off = true; };
  }, [id, tick, nav.baskets]);

  React.useEffect(() => {
    let off = false;
    setTree(null);
    const p = new URLSearchParams();
    if (path.length) p.set("path", path.map(encodeURIComponent).join("/"));
    if (qDeb) p.set("q", qDeb);
    api<VTree>(`/vbaskets/${id}/tree?${p}`).then((t) => { if (!off) setTree(t); }).catch((e) => { if (!off) { setErr(e.message); setTree(null); } });
    return () => { off = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, sp.get("path"), qDeb, tick]);

  const go = (params: Record<string, string | null>) => {
    const p = new URLSearchParams(sp.toString());
    for (const [k, v] of Object.entries(params)) { if (v == null || v === "") p.delete(k); else p.set(k, v); }
    router.push(`${pathname}${p.toString() ? `?${p}` : ""}`);
  };
  const pathTo = (n: number, extra?: string | null) => {
    const parts = path.slice(0, n);
    if (extra !== undefined) parts.push(extra ?? NONE);
    return parts.map(encodeURIComponent).join("/");
  };
  const addToDownloads = async (items: DlItem[]) => {
    const cur = await api<{ item: DlItem }[]>("/me/downloads");
    const have = cur.map((c) => c.item);
    const next = [...have, ...items.filter((i) => !have.some((h) => JSON.stringify(h) === JSON.stringify(i)))];
    await api("/me/downloads", { method: "PUT", json: next });
    nav.setDownloads(next.length);
    toast({ text: `Added to your download list.` });
  };

  if (err && !vb) return <div className="cx-page"><Note kind="error">{err}</Note></div>;
  const atFiles = tree && tree.level >= (vb?.group_by.length ?? 0);
  const canUpload = !!vb?.drop_target_system_id;

  return (
    <div className="cx-page">
      <div className="cx-crumbs">
        {path.length > 0 && vb && <><Link href={`/v/${id}`}>{vb.name}</Link>
          {tree?.path.slice(0, -1).map((c, i) => <React.Fragment key={i}><span>/</span><Link href={`/v/${id}?path=${pathTo(i + 1)}`}>{c.label}</Link></React.Fragment>)}</>}
      </div>
      <div className="cx-title-row">
        <h1 className="cx-title">{path.length && tree ? tree.path[tree.path.length - 1]?.label : vb?.name ?? " "}</h1>
        <Tag color="blue">Virtual</Tag>
        {!path.length && vb && (
          <button className="cx-star" aria-pressed={nav.stars.includes(`vbasket:${id}`)}
            aria-label={nav.stars.includes(`vbasket:${id}`) ? "Remove star" : "Star to show in the side menu"}
            onClick={() => nav.toggleStar("vbasket", id)}>{nav.stars.includes(`vbasket:${id}`) ? <I.starFilled /> : <I.star />}</button>
        )}
        <span className="cx-sub cx-num">{tree ? `${tree.total} file${tree.total === 1 ? "" : "s"} · ${bytes(tree.bytes)}` : ""}</span>
        <span className="cx-spacer" />
        {vb?.can_edit && <Btn kind="ghost" onClick={() => router.push(`/v/${id}/edit`)}>Edit rules</Btn>}
        {vb?.can_edit && <Btn kind="danger-ghost" onClick={() => setModal(<Confirm title="Delete this virtual basket?" danger action="Delete"
          body={`${vb.name} is only a view. The files it shows stay in their baskets.`}
          onClose={() => setModal(null)} onConfirm={async () => {
            await api(`/vbaskets/${id}`, { method: "DELETE" });
            toast({ text: `Virtual basket ${vb.name} deleted. Its files stay in their baskets.` });
            nav.reload(); router.push("/recent");
          }} />)}>Delete</Btn>}
      </div>
      {vb && <div className="cx-sub">{vb.source_names.join(", ")} · {describeRules(vb.match_mode, vb.rules) || "All files"}{vb.group_by.length ? ` · grouped by ${vb.group_by_labels.join(" › ")}` : ""}</div>}

      <div className={`cx-work${fileParam ? " cx-has-panel" : ""}`}>
        <div className="cx-list-wrap">
          <div className="cx-toolbar">
            <div className="cx-grow cx-search-wrap"><I.search />
              <input className="cx-search" placeholder="Filter within this view" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Filter within this view" /></div>
            <Btn kind="ghost" onClick={() => addToDownloads([{ kind: "vgroup", vbasket_id: id, path }])}>Add to download list</Btn>
            {canUpload && <Btn kind="primary" icon={<I.upload />} onClick={() => setModal(
              <UploadModal basket={vb!.drop_target_system_id!} folder={vb!.drop_target_folder_id} folderLabel={vb!.drop_target_label ?? ""}
                title={`Upload to ${vb!.drop_target_label}`} settings={dropSettings} prefill={vb!.prefill}
                onClose={() => setModal(null)} onDone={refresh} />)}>Upload</Btn>}
          </div>
          <div className="cx-table-scroll">
            <table className="cx-table">
              <thead><tr>
                <th>Name</th><th className="cx-hide-sm">Basket</th><th>Status</th><th>Modified</th>
                <th className="cx-right cx-hide-sm">Files</th><th className="cx-right cx-hide-sm">Size</th><th style={{ width: 32 }} />
              </tr></thead>
              <tbody>
                {!tree && <SkeletonRows cols={7} />}
                {tree?.groups.map((g) => (
                  <tr key={g.value ?? NONE}>
                    <td className="cx-name"><Link className="cx-filelink" href={`/v/${id}?path=${pathTo(path.length, g.value)}`}><I.vfolder /><span>{g.label}</span></Link></td>
                    <td className="cx-hide-sm" /><td />
                    <td className="cx-num">{when(g.modified_at, false)}</td>
                    <td className="cx-right cx-num cx-hide-sm">{g.count}</td>
                    <td className="cx-right cx-num cx-hide-sm">{bytes(g.bytes)}</td>
                    <td><OverflowMenu items={[
                      { label: "Open", onClick: () => go({ path: pathTo(path.length, g.value) }) },
                      { label: "Add to download list", onClick: () => addToDownloads([{ kind: "vgroup", vbasket_id: id, path: [...path, g.value ?? NONE] }]) },
                    ]} /></td>
                  </tr>
                ))}
                {atFiles && tree?.items.map((f) => (
                  <tr key={f.id} aria-selected={fileParam === f.id}>
                    <td className="cx-name"><button className="cx-filelink" onClick={() => go({ file: f.id })}><span className="cx-ftype">{ext(f.name)}</span><span>{f.name}</span></button></td>
                    <td className="cx-hide-sm"><Link href={`/b/${f.system_id}${f.folder_id ? `?folder=${f.folder_id}` : ""}`} style={{ color: "inherit" }}>{f.basket_name}</Link></td>
                    <td><StatusTag code={f.status} statuses={statuses} /></td>
                    <td><When iso={f.modified_at} /></td>
                    <td className="cx-hide-sm" />
                    <td className="cx-right cx-num cx-hide-sm">{bytes(f.size_bytes)}</td>
                    <td><OverflowMenu items={[
                      { label: "Open", onClick: () => router.push(`/view/${f.id}`) },
                      { label: "Details", onClick: () => go({ file: f.id }) },
                      { label: "Download", onClick: () => { window.location.href = `${BASE_PATH}/api/files/${f.id}/download`; } },
                      { label: "Add to download list", onClick: () => addToDownloads([{ kind: "file", id: f.id }]) },
                      { label: "Open in basket", onClick: () => router.push(`/b/${f.system_id}?${f.folder_id ? `folder=${f.folder_id}&` : ""}file=${f.id}`) },
                      ...(vb?.can_edit ? ["sep" as const, { label: "Remove from this view", onClick: async () => {
                        await api(`/vbaskets/${id}/excludes`, { method: "POST", json: { file_id: f.id } });
                        toast({ text: `${f.name} no longer shows here. It stays in ${f.basket_name}.`, undo: async () => { await api(`/vbaskets/${id}/excludes/${f.id}`, { method: "DELETE" }); refresh(); } });
                        refresh();
                      } }] : []),
                    ]} /></td>
                  </tr>
                ))}
              </tbody>
            </table>
            {tree && tree.groups.length === 0 && tree.items.length === 0 && (
              <div className="cx-state"><strong>{qDeb ? "Nothing matches this filter." : "No files match these rules."}</strong></div>
            )}
          </div>
        </div>
        {fileParam && <>
          <div className="cx-panel-scrim" onClick={() => go({ file: null })} />
          <FilePanel fileId={fileParam} settingsFor={loadSettings} refreshKey={tick} onClose={() => go({ file: null })} onChanged={refresh}
            onShare={(f) => setModal(<ShareModal rtype="file" rid={f.id} name={f.name} onClose={() => setModal(null)} />)}
            onStatus={(f) => setModal(<StatusModal fileId={f.id} fileName={f.name} onClose={() => setModal(null)} onDone={refresh} />)} />
        </>}
      </div>
      {modal}
    </div>
  );
}

// ── FH-03 Rules editor ────────────────────────────────────────────────────

type Cond = { field: string; op: string; value: unknown };
type Row = Cond | { match: "all" | "any"; rules: Cond[] };
const isGroup = (r: Row): r is { match: "all" | "any"; rules: Cond[] } => "rules" in r;
/// A rule is sent only once it has a field and (unless empty/not empty) a value.
const complete = (c: Cond) => !!c.field && (c.op === "empty" || c.op === "not_empty" ||
  (Array.isArray(c.value) ? c.value.length > 0 : c.value != null && String(c.value).trim() !== ""));

function opsFor(kind: string): { value: string; label: string }[] {
  const base = [{ value: "empty", label: "is empty" }, { value: "not_empty", label: "is not empty" }];
  switch (kind) {
    case "date": return [{ value: "after", label: "after" }, { value: "before", label: "before" }, { value: "last_days", label: "in last N days" }, ...base];
    case "number": return [{ value: "eq", label: "=" }, { value: "ne", label: "≠" }, { value: "gt", label: ">" }, { value: "gte", label: "≥" }, { value: "lt", label: "<" }, { value: "lte", label: "≤" }, ...base];
    case "choice": case "category": case "status": case "basket": case "org":
      return [{ value: "in", label: "is any of" }, { value: "nin", label: "is none of" }, { value: "eq", label: "is" }, { value: "ne", label: "is not" }, ...base];
    default: return [{ value: "eq", label: "is" }, { value: "ne", label: "is not" }, { value: "contains", label: "contains" }, { value: "starts", label: "starts with" }, { value: "in", label: "is any of" }, ...base];
  }
}

export function RulesEditor({ id }: { id: string | null }) {
  const router = useRouter();
  const nav = useNav();
  const { user } = useAuth();
  const toast = useToast();
  const [name, setName] = React.useState("");
  const [sources, setSources] = React.useState<string[]>([]);
  const [allSources, setAllSources] = React.useState(false);
  const [mode, setMode] = React.useState<"all" | "any">("all");
  const [rows, setRows] = React.useState<Row[]>([{ field: "sys.format", op: "in", value: [] }]);
  const [groupBy, setGroupBy] = React.useState<string[]>([]);
  const [drop, setDrop] = React.useState<{ sys: string; folder: string | null }>({ sys: "", folder: null });
  const [dropFolders, setDropFolders] = React.useState<Folder[]>([]);
  const [personal, setPersonal] = React.useState(user?.role === "viewer");
  const [fields, setFields] = React.useState<FieldOption[]>([]);
  const [preview, setPreview] = React.useState<{ count: number; bytes: number; items: { id: string; name: string; basket_name: string; size_bytes: number }[] } | null>(null);
  const [pErr, setPErr] = React.useState<string | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [confirmDel, setConfirmDel] = React.useState(false);
  const baskets: Basket[] = [...(nav.drive ? [nav.drive] : []), ...nav.baskets];

  React.useEffect(() => {
    if (!id) return;
    api<VBasket>(`/vbaskets/${id}`).then((v) => {
      setName(v.name); setSources(v.sources); setAllSources(v.all_sources); setMode(v.match_mode);
      setRows((v.rules as Row[]).length ? (v.rules as Row[]) : []); setGroupBy(v.group_by);
      setDrop({ sys: v.drop_target_system_id ?? "", folder: v.drop_target_folder_id }); setPersonal(v.personal);
    }).catch((e) => setErr(e.message));
  }, [id]);
  React.useEffect(() => {
    const p = allSources || sources.length === 0 ? "all=true" : `sources=${sources.join(",")}`;
    api<FieldOption[]>(`/vbaskets/fields?${p}`).then(setFields).catch(() => setFields([]));
  }, [sources, allSources]);
  React.useEffect(() => {
    if (!drop.sys) { setDropFolders([]); return; }
    api<Folder[]>(`/folders?system_id=${drop.sys}`).then(setDropFolders).catch(() => setDropFolders([]));
  }, [drop.sys]);

  const body = () => ({
    name: name.trim() || "Untitled virtual basket", sources: allSources ? [] : sources, all_sources: allSources, match_mode: mode,
    rules: rows.map((r) => isGroup(r) ? { match: r.match, rules: r.rules.filter(complete) } : r).filter((r) => isGroup(r) ? r.rules.length : complete(r)) as Rule[],
    group_by: groupBy, drop_target_system_id: drop.sys || null, drop_target_folder_id: drop.sys ? drop.folder : null, personal,
  });
  React.useEffect(() => {
    if (!allSources && sources.length === 0) { setPreview(null); setPErr(null); return; }
    const t = setTimeout(() => {
      api<typeof preview>("/vbaskets/preview", { method: "POST", json: body() }).then((p) => { setPreview(p); setPErr(null); }).catch((e) => { setPreview(null); setPErr(e.message); });
    }, 350);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sources, allSources, mode, rows]);

  const fieldOpts = fields.map((f) => ({ value: f.key, label: f.label, hint: f.system ? undefined : "field" }));
  const fOf = (k: string) => fields.find((f) => f.key === k);
  const save = async () => {
    setErr(null);
    try {
      const v = await api<VBasket>(id ? `/vbaskets/${id}` : "/vbaskets", { method: id ? "PUT" : "POST", json: body() });
      toast({ text: `Virtual basket ${v.name} saved.` }); nav.reload(); router.push(`/v/${v.id}`);
    } catch (e) { setErr((e as Error).message); }
  };

  const condRow = (c: Cond, set: (c: Cond | null) => void) => {
    const f = fOf(c.field);
    const kind = f?.kind ?? "text";
    const ops = opsFor(kind);
    const multi = c.op === "in" || c.op === "nin";
    const noVal = c.op === "empty" || c.op === "not_empty";
    return (
      <div className="cx-rule">
        <Dropdown label="Field" value={c.field} options={fieldOpts} searchable onChange={(v) => set({ field: v, op: opsFor(fOf(v)?.kind ?? "text")[0].value, value: undefined })} />
        <Dropdown label="Condition" value={c.op} options={ops} onChange={(v) => set({ ...c, op: v, value: v === "in" || v === "nin" ? [] : undefined })} />
        {noVal ? <div /> : f && f.choices.length > 0 ? (
          multi ? <Dropdown label="Values" multi values={(c.value as string[]) ?? []} options={f.choices} onChangeMulti={(v) => set({ ...c, value: v })} placeholder="Choose values" />
            : <Dropdown label="Value" value={String(c.value ?? "")} options={f.choices} onChange={(v) => set({ ...c, value: v })} />
        ) : (
          <TextField label={c.op === "last_days" ? "Days" : "Value"} type={kind === "date" && c.op !== "last_days" ? "date" : "text"}
            value={Array.isArray(c.value) ? (c.value as string[]).join(", ") : String(c.value ?? "")}
            helper={multi ? "Separate values with commas" : undefined}
            onChange={(e) => set({ ...c, value: multi ? e.target.value.split(",").map((x) => x.trim()).filter(Boolean) : e.target.value })} />
        )}
        <button className="cx-close" aria-label="Remove rule" onClick={() => set(null)}><I.close /></button>
      </div>
    );
  };

  return (
    <div className="cx-page" style={{ maxWidth: 960 }}>
      <div className="cx-crumbs">{id ? <Link href={`/v/${id}`}>{name || "Virtual basket"}</Link> : <span>Virtual baskets</span>}</div>
      <div className="cx-title-row"><h1 className="cx-title">{id ? "Edit virtual basket" : "New virtual basket"}</h1><Tag color="blue">Virtual</Tag></div>
      <div style={{ background: "var(--cx-layer)", padding: 16, display: "flex", flexDirection: "column", gap: 16 }} className="cx-on-layer">
        <div className="cx-form-grid">
          <TextField label="Name" required value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Justification PDFs 2027" />
          <Dropdown label="Sources" required multi values={sources} disabled={allSources} placeholder={allSources ? "All baskets" : "Choose baskets"}
            options={baskets.map((b) => ({ value: b.id, label: b.system_type === "personal" ? "My Drive" : b.name }))} onChangeMulti={setSources} />
          <div className="cx-span" style={{ display: "flex", gap: 24, flexWrap: "wrap" }}>
            <Check label="Use every basket I can see" checked={allSources} onChange={setAllSources} />
            <Check label="Only I can see this virtual basket" checked={personal} onChange={setPersonal} />
          </div>
        </div>

        <div>
          <div style={{ display: "flex", alignItems: "end", gap: 12, marginBottom: 8 }}>
            <div style={{ fontWeight: 600, flex: 1 }}>Rules</div>
            <Dropdown value={mode} options={[{ value: "all", label: "Match all" }, { value: "any", label: "Match any" }]} onChange={(v) => setMode(v as "all" | "any")} />
          </div>
          {rows.map((r, i) => isGroup(r) ? (
            <div key={i} className="cx-rule-group" style={{ margin: "4px 0" }}>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <Dropdown value={r.match} options={[{ value: "all", label: "All of" }, { value: "any", label: "Any of" }]}
                  onChange={(v) => setRows((x) => x.map((y, j) => j === i ? { ...r, match: v as "all" | "any" } : y))} />
                <span className="cx-spacer" style={{ flex: 1 }} />
                <Btn kind="danger-ghost" onClick={() => setRows((x) => x.filter((_, j) => j !== i))}>Remove group</Btn>
              </div>
              {r.rules.map((c, k) => <React.Fragment key={k}>{condRow(c, (n) => setRows((x) => x.map((y, j) => j !== i ? y : { ...r, rules: n ? r.rules.map((z, m) => m === k ? n : z) : r.rules.filter((_, m) => m !== k) })))}</React.Fragment>)}
              <div><Btn kind="ghost" onClick={() => setRows((x) => x.map((y, j) => j === i ? { ...r, rules: [...r.rules, { field: "sys.format", op: "in", value: [] }] } : y))}>Add rule to group</Btn></div>
            </div>
          ) : <React.Fragment key={i}>{condRow(r, (n) => setRows((x) => n ? x.map((y, j) => j === i ? n : y) : x.filter((_, j) => j !== i)))}</React.Fragment>)}
          <div style={{ display: "flex", gap: 0, marginTop: 4 }}>
            <Btn kind="ghost" onClick={() => setRows((x) => [...x, { field: "sys.format", op: "in", value: [] }])}>Add rule</Btn>
            <Btn kind="ghost" onClick={() => setRows((x) => [...x, { match: "any", rules: [{ field: "sys.status", op: "eq", value: "" }] }])}>Add group</Btn>
          </div>
        </div>

        <div className="cx-form-grid">
          <div className="cx-field">
            <span className="cx-label">Group by (folder levels, in order)</span>
            <div className="cx-order">
              {groupBy.map((g, i) => (
                <div key={g}>
                  <span>{i + 1}. {fOf(g)?.label ?? g}</span>
                  <button className="cx-close" aria-label="Move up" disabled={i === 0} onClick={() => setGroupBy((x) => { const y = [...x]; [y[i - 1], y[i]] = [y[i], y[i - 1]]; return y; })}><I.arrowUp size={12} /></button>
                  <button className="cx-close" aria-label="Move down" disabled={i === groupBy.length - 1} onClick={() => setGroupBy((x) => { const y = [...x]; [y[i + 1], y[i]] = [y[i], y[i + 1]]; return y; })}><I.arrowDown size={12} /></button>
                  <button className="cx-close" aria-label="Remove level" onClick={() => setGroupBy((x) => x.filter((_, j) => j !== i))}><I.close size={12} /></button>
                </div>
              ))}
              {groupBy.length < 4 && <Dropdown value="" placeholder="Add level" searchable
                options={fields.filter((f) => !groupBy.includes(f.key) && !["sys.name", "sys.tag", "sys.size", "sys.created_at", "sys.modified_at"].includes(f.key)).map((f) => ({ value: f.key, label: f.label }))}
                onChange={(v) => setGroupBy((x) => [...x, v])} />}
            </div>
          </div>
          <div className="cx-field">
            <Dropdown label="Drop target (where uploads from here go)" value={drop.sys} placeholder="No uploads from this view"
              options={[{ value: "", label: "No uploads from this view" }, ...baskets.filter((b) => allSources || sources.includes(b.id)).map((b) => ({ value: b.id, label: b.system_type === "personal" ? "My Drive" : b.name }))]}
              onChange={(v) => setDrop({ sys: v, folder: null })} />
            {drop.sys && <Dropdown label="Folder" value={drop.folder ?? ""} options={[{ value: "", label: "Basket root" }, ...dropFolders.map((f) => ({ value: f.id, label: f.name }))]}
              onChange={(v) => setDrop((d) => ({ ...d, folder: v || null }))} />}
          </div>
        </div>

        <div>
          <div className="cx-label" style={{ marginBottom: 4 }}>Preview{preview ? ` · ${preview.count} file${preview.count === 1 ? "" : "s"} match · ${bytes(preview.bytes)}` : ""}</div>
          {pErr && <Note kind="error">{pErr}</Note>}
          {!pErr && !preview && <div className="cx-helper">Choose at least one source to see matching files.</div>}
          {preview && (
            <ul className="cx-list" style={{ background: "var(--cx-layer-2)", padding: "0 12px" }}>
              {preview.items.length === 0 && <li className="cx-sub">No files match these rules yet.</li>}
              {preview.items.map((i) => <li key={i.id}><span className="cx-grow">{i.name}</span><span className="cx-sub">{i.basket_name}</span><span className="cx-sub cx-num" style={{ width: 64, textAlign: "right" }}>{bytes(i.size_bytes)}</span></li>)}
            </ul>
          )}
        </div>
        {err && <Note kind="error">{err}</Note>}
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <Btn kind="primary" onClick={save}>Save</Btn>
          <Btn kind="secondary" onClick={() => router.back()}>Cancel</Btn>
          <span style={{ flex: 1 }} />
          {id && <Btn kind="danger-ghost" onClick={() => setConfirmDel(true)}>Delete virtual basket</Btn>}
        </div>
      </div>
      {confirmDel && id && <Confirm title="Delete this virtual basket?" danger action="Delete" body="Files stay in their baskets. Only this view goes away."
        onClose={() => setConfirmDel(false)} onConfirm={async () => { await api(`/vbaskets/${id}`, { method: "DELETE" }); nav.reload(); router.push("/recent"); }} />}
    </div>
  );
}

