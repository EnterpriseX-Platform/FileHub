"use client";

import * as React from "react";

import { uploadChunked } from "@/lib/chunked";

import {
  api, bytes, choiceList, when, BASE_PATH,
  type AclResponse, type Basket, type Category, type FieldDef, type Folder, type Principal, type Settings,
} from "@/lib/fh";
import { Btn, Check, Dropdown, I, Modal, Note, Tag, TextArea, TextField, useToast } from "./ui";
import { UploadLinks } from "./upload-links";
import { PublicLinks } from "./public-links";

// ── Field form (driven by the basket's field definitions) ────────────────
export function FieldInputs({ fields, values, onChange, locked = [], categories, category, onCategory, errors }: {
  fields: FieldDef[]; values: Record<string, string>; onChange: (k: string, v: string) => void; locked?: string[];
  categories?: Category[]; category?: string; onCategory?: (c: string) => void; errors?: Record<string, string>;
}) {
  return (
    <div className="cx-form-grid">
      {categories && categories.length > 0 && onCategory && (
        <Dropdown label="Category" value={category ?? ""} placeholder="No category" disabled={locked.includes("category_code")}
          options={[{ value: "", label: "No category" }, ...categories.map((c) => ({ value: c.code, label: c.name }))]} onChange={onCategory} />
      )}
      {fields.map((f) => {
        const dis = locked.includes(f.key);
        const err = errors?.[f.key];
        if (f.field_type === "choice") {
          return <Dropdown key={f.key} label={f.label} required={f.required} value={values[f.key] ?? ""} disabled={dis}
            options={[...(f.required ? [] : [{ value: "", label: "—" }]), ...choiceList(f)]} onChange={(v) => onChange(f.key, v)} />;
        }
        return <TextField key={f.key} label={f.label} required={f.required} value={values[f.key] ?? ""} disabled={dis} error={err}
          type={f.field_type === "number" ? "text" : f.field_type === "date" ? "date" : "text"}
          inputMode={f.field_type === "number" ? "decimal" : undefined}
          onChange={(e) => onChange(f.key, e.target.value)} />;
      })}
    </div>
  );
}

export function cleanFields(fields: FieldDef[], values: Record<string, string>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(values)) {
    if (v === "" || v == null) continue;
    const def = fields.find((f) => f.key === k);
    out[k] = def?.field_type === "number" && /^-?\d+(\.\d+)?$/.test(v) ? Number(v) : v;
  }
  return out;
}

function missingRequired(fields: FieldDef[], values: Record<string, string>): Record<string, string> {
  const e: Record<string, string> = {};
  for (const f of fields) if (f.required && !(values[f.key] ?? "").trim()) e[f.key] = `${f.label} is required.`;
  return e;
}

// ── Upload ────────────────────────────────────────────────────────────────
type QItem = { file: File; progress: number; error?: string; done?: boolean; dir?: string };

/// Largest file when the basket sets no limit (the server applies the same default).
export const DEFAULT_MAX_FILE_BYTES = 20 * 1024 * 1024;

/// Every file inside dropped folders, with the folder path it sits in.
async function filesFromDrop(dt: DataTransfer): Promise<{ file: File; dir: string }[]> {
  type Ent = { isFile: boolean; isDirectory: boolean; name: string; file?: (ok: (f: File) => void, bad: (e: unknown) => void) => void;
    createReader?: () => { readEntries: (ok: (e: Ent[]) => void, bad: (e: unknown) => void) => void } };
  const roots = Array.from(dt.items).map((i) => (i as DataTransferItem & { webkitGetAsEntry?: () => Ent | null }).webkitGetAsEntry?.()).filter(Boolean) as Ent[];
  if (!roots.some((e) => e.isDirectory)) return Array.from(dt.files).map((file) => ({ file, dir: "" }));
  const out: { file: File; dir: string }[] = [];
  const walk = async (e: Ent, dir: string): Promise<void> => {
    if (e.isFile && e.file) { const f = await new Promise<File>((ok, bad) => e.file!(ok, bad)); out.push({ file: f, dir }); return; }
    if (e.isDirectory && e.createReader) {
      const r = e.createReader(); const sub = dir ? `${dir}/${e.name}` : e.name;
      for (;;) { const batch = await new Promise<Ent[]>((ok, bad) => r.readEntries(ok, bad)); if (!batch.length) break; for (const c of batch) await walk(c, sub); }
    }
  };
  for (const r of roots) await walk(r, "");
  return out;
}

export function UploadModal({ basket, folder, folderLabel, settings, prefill, title, onClose, onDone, initialFiles }: {
  basket: string; folder: string | null; folderLabel: string; settings: Settings | null;
  prefill?: { category_code?: string; fields?: Record<string, string> }; title?: string;
  onClose: () => void; onDone: () => void; initialFiles?: File[];
}) {
  const toast = useToast();
  const [queue, setQueue] = React.useState<QItem[]>([]);
  const [cat, setCat] = React.useState(prefill?.category_code ?? "");
  const [vals, setVals] = React.useState<Record<string, string>>(() => ({ ...(prefill?.fields ?? {}) }));
  const [errs, setErrs] = React.useState<Record<string, string>>({});
  const [busy, setBusy] = React.useState(false);
  const [over, setOver] = React.useState(false);
  const input = React.useRef<HTMLInputElement>(null);
  const dirInput = React.useRef<HTMLInputElement>(null);
  const fields = settings?.fields ?? [];
  const policy = settings?.policy;
  const locked = [...Object.keys(prefill?.fields ?? {}), ...(prefill?.category_code ? ["category_code"] : [])];

  const precheck = (f: File): string | undefined => {
    const ext = (/\.([^./]+)$/.exec(f.name)?.[1] ?? "").toLowerCase();
    if (policy?.allowed_ext && policy.allowed_ext.length && !policy.allowed_ext.includes(ext))
      return `ไม่อนุญาตไฟล์ .${ext || "(ไม่มีนามสกุล)"} ในตะกร้านี้`;
    const max = policy?.max_file_bytes == null ? DEFAULT_MAX_FILE_BYTES : policy.max_file_bytes;
    if (max > 0 && f.size > max) return `ไฟล์เกินขนาดสูงสุด ${bytes(max)}`;
    return undefined;
  };
  const addWithDirs = (items: { file: File; dir: string }[]) =>
    setQueue((q) => [...q, ...items.map(({ file, dir }) => ({ file, dir, progress: 0, error: precheck(file) }))]);
  const add = (files: FileList | File[]) => addWithDirs(Array.from(files).map((f) => {
    const rel = (f as File & { webkitRelativePath?: string }).webkitRelativePath ?? "";
    return { file: f, dir: rel.includes("/") ? rel.slice(0, rel.lastIndexOf("/")) : "" };
  }));
  // Folder path inside the basket → folder id (made on the way when missing).
  const made = React.useRef(new Map<string, string | null>([["", folder]]));
  const ensureDir = async (dir: string): Promise<string | null> => {
    if (made.current.has(dir)) return made.current.get(dir) ?? null;
    const i = dir.lastIndexOf("/");
    const parent = await ensureDir(i < 0 ? "" : dir.slice(0, i));
    const name = i < 0 ? dir : dir.slice(i + 1);
    const q = `/folders?system_id=${encodeURIComponent(basket)}${parent ? `&parent_id=${encodeURIComponent(parent)}` : ""}`;
    const existing = (await api<{ id: string; name: string; parent_id: string | null }[]>(q)).find((x) => x.name === name && (x.parent_id ?? null) === parent);
    const id = existing ? existing.id : (await api<{ id: string }>("/folders", { method: "POST", json: { name, system_id: basket, parent_id: parent } })).id;
    made.current.set(dir, id);
    return id;
  };
  // Files dropped on the list arrive here — check them like any other.
  const seeded = React.useRef(false);
  React.useEffect(() => { if (!seeded.current && initialFiles?.length) { seeded.current = true; add(initialFiles); } }, [initialFiles]); // eslint-disable-line react-hooks/exhaustive-deps
  const ok = queue.filter((q) => !q.error && !q.done);

  const send = async (item: QItem, idx: number) => {
    const set = (patch: Partial<QItem>) => setQueue((q) => q.map((x, i) => i === idx ? { ...x, ...patch } : x));
    try {
      const target = item.dir ? await ensureDir(item.dir) : folder;
      await uploadChunked({
        endpoint: "/api/uploads", file: item.file,
        meta: { system_id: basket, folder_id: target || undefined, category_code: cat || undefined, fields: JSON.stringify(cleanFields(fields, vals)) },
        onProgress: (n, t) => set({ progress: t ? Math.round((n / t) * 100) : 100 }),
      });
      set({ progress: 100, done: true, error: undefined });
      return true;
    } catch (e) { set({ error: (e as Error).message }); return false; }
  };

  const start = async () => {
    const e = missingRequired(fields, vals);
    setErrs(e);
    if (Object.keys(e).length) return;
    setBusy(true);
    let done = 0, failed = queue.filter((q) => q.error).length;
    for (let i = 0; i < queue.length; i++) {
      if (queue[i].error || queue[i].done) continue;
      if (await send(queue[i], i)) done++; else failed++;
    }
    setBusy(false);
    if (done) { toast({ text: `Uploaded ${done} file${done > 1 ? "s" : ""} to ${folderLabel}.` }); onDone(); }
    if (!failed) onClose();
  };

  return (
    <Modal title={title ?? `Upload to ${folderLabel}`} size="wide" onClose={onClose} initialFocus={false} footer={<>
      <Btn kind="secondary" onClick={onClose}>{queue.some((q) => q.done) ? "Close" : "Cancel"}</Btn>
      <Btn kind="primary" disabled={busy || ok.length === 0} onClick={start}>
        {busy ? "Uploading…" : `Upload ${ok.length} file${ok.length === 1 ? "" : "s"}`}
      </Btn>
    </>}>
      <div className="cx-drop" data-over={over} role="button" tabIndex={0}
        onClick={() => input.current?.click()}
        onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); input.current?.click(); } }}
        onDragOver={(e) => { e.preventDefault(); setOver(true); }} onDragLeave={() => setOver(false)}
        onDrop={(e) => { e.preventDefault(); setOver(false); const dt = e.dataTransfer; filesFromDrop(dt).then(addWithDirs).catch(() => add(dt.files)); }}>
        ลากไฟล์หรือโฟลเดอร์มาวางที่นี่ หรือ <b>เลือกไฟล์</b> · <b role="button" tabIndex={0} onClick={(e) => { e.stopPropagation(); dirInput.current?.click(); }}
          onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.stopPropagation(); dirInput.current?.click(); } }}>เลือกโฟลเดอร์</b>
        {policy?.allowed_ext?.length ? <div className="cx-helper" style={{ marginTop: 4 }}>{policy.allowed_ext.map((x) => `.${x}`).join(" ")}{policy.max_file_bytes ? ` · up to ${bytes(policy.max_file_bytes)}` : ""}</div> : null}
        <input ref={input} type="file" multiple hidden onChange={(e) => { if (e.target.files) add(e.target.files); e.target.value = ""; }} />
        <input ref={dirInput} type="file" multiple hidden {...({ webkitdirectory: "", directory: "" } as Record<string, string>)}
          onClick={(e) => e.stopPropagation()} onChange={(e) => { if (e.target.files) add(e.target.files); e.target.value = ""; }} />
      </div>
      {queue.length > 0 && (
        <ul className="cx-list" style={{ background: "var(--cx-layer-2)", padding: "0 12px" }}>
          {queue.map((q, i) => (
            <li key={i}>
              <span className="cx-grow">{q.dir ? `${q.dir}/` : ""}{q.file.name}</span>
              <span className="cx-num cx-sub" style={{ width: 64, textAlign: "right" }}>{bytes(q.file.size)}</span>
              <span style={{ width: 160 }}>
                {q.error ? <span className="cx-error">{q.error}</span>
                  : q.done ? <Tag color="green">Uploaded</Tag>
                  : <span style={{ display: "flex", alignItems: "center", gap: 6 }}><span className="cx-progress" style={{ flex: 1 }}><i style={{ width: `${q.progress}%` }} /></span><span className="cx-num cx-sub">{q.progress}%</span></span>}
              </span>
              {!busy && !q.done && <button className="cx-close" style={{ width: 24, height: 24 }} aria-label={`Remove ${q.file.name}`} onClick={() => setQueue((x) => x.filter((_, j) => j !== i))}><I.close size={12} /></button>}
            </li>
          ))}
        </ul>
      )}
      {(fields.length > 0 || (settings?.categories.length ?? 0) > 0) && (
        <FieldInputs fields={fields} values={vals} errors={errs} locked={locked}
          onChange={(k, v) => setVals((x) => ({ ...x, [k]: v }))}
          categories={settings?.categories} category={cat} onCategory={setCat} />
      )}
    </Modal>
  );
}

// ── New folder / rename ───────────────────────────────────────────────────
export function NameModal({ title, action, initial = "", onSubmit, onClose }: {
  title: string; action: string; initial?: string; onSubmit: (name: string) => Promise<void>; onClose: () => void;
}) {
  const [v, setV] = React.useState(initial);
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const go = async () => {
    if (!v.trim()) { setErr("Enter a name."); return; }
    setBusy(true); setErr(null);
    try { await onSubmit(v.trim()); onClose(); } catch (e) { setErr((e as Error).message); setBusy(false); }
  };
  return (
    <Modal title={title} size="narrow" onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind="primary" disabled={busy} onClick={go}>{action}</Btn>
    </>}>
      <form onSubmit={(e) => { e.preventDefault(); go(); }}>
        <TextField label="Name" value={v} error={err} onChange={(e) => setV(e.target.value)} />
      </form>
    </Modal>
  );
}

// ── New folder with its options (versions, encryption) ────────────────────
export function NewFolderModal({ onSubmit, onClose }: {
  onSubmit: (v: { name: string; versioning: boolean; encrypted: boolean }) => Promise<void>; onClose: () => void;
}) {
  const [v, setV] = React.useState("");
  const [ver, setVer] = React.useState("on");
  const [enc, setEnc] = React.useState("none");
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const go = async () => {
    if (!v.trim()) { setErr("กรุณากรอกชื่อโฟลเดอร์"); return; }
    setBusy(true); setErr(null);
    try { await onSubmit({ name: v.trim(), versioning: ver === "on", encrypted: enc === "aes" }); onClose(); } catch (e) { setErr((e as Error).message); setBusy(false); }
  };
  return (
    <Modal title="สร้างโฟลเดอร์" size="narrow" onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>ยกเลิก</Btn>
      <Btn kind="primary" disabled={busy} onClick={go}>สร้างโฟลเดอร์</Btn>
    </>}>
      <form onSubmit={(e) => { e.preventDefault(); go(); }} style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        <TextField label="ชื่อโฟลเดอร์" value={v} error={err} onChange={(e) => setV(e.target.value)} />
        <Dropdown label="การควบคุมเวอร์ชัน" value={ver} options={[{ value: "on", label: "เก็บทุกเวอร์ชัน" }, { value: "off", label: "ไม่เก็บเวอร์ชันเก่า" }]} onChange={setVer} />
        <Dropdown label="การเข้ารหัส" value={enc} options={[{ value: "none", label: "ตามค่าของตะกร้า" }, { value: "aes", label: "เข้ารหัสแบบสมมาตร (AES-256)" }]} onChange={setEnc} />
      </form>
    </Modal>
  );
}

// ── Folder details ────────────────────────────────────────────────────────
type FolderInfo = { name: string; owner: string; creator_name: string; creator_email: string | null; created_at: string;
  file_count: number; subfolder_count: number; total_bytes: number; encrypted: boolean };
export function FolderDetailsModal({ folderId, onClose }: { folderId: string; onClose: () => void }) {
  const [d, setD] = React.useState<FolderInfo | null>(null);
  const [pol, setPol] = React.useState<{ effective: { encrypt: boolean | null; keep_versions?: boolean | null } } | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  React.useEffect(() => {
    api<FolderInfo>(`/folders/${folderId}`).then(setD).catch((e) => setErr((e as Error).message));
    api<{ effective: { encrypt: boolean | null; keep_versions?: boolean | null } }>(`/folders/${folderId}/policy`).then(setPol).catch(() => {});
  }, [folderId]);
  return (
    <Modal title="รายละเอียดโฟลเดอร์" size="narrow" onClose={onClose} footer={<Btn kind="secondary" onClick={onClose}>ปิด</Btn>}>
      {err && <Note kind="error">{err}</Note>}
      {d && <dl className="cx-kv">
        <dt>ชื่อ</dt><dd>{d.name}</dd>
        <dt>ผู้สร้าง</dt><dd>{d.creator_name}{d.creator_email ? ` (${d.creator_email})` : ""}</dd>
        <dt>วันเวลาที่สร้าง</dt><dd className="cx-num">{when(d.created_at)}</dd>
        <dt>ขนาดรวม</dt><dd className="cx-num">{bytes(d.total_bytes) || "0 B"}</dd>
        <dt>จำนวนไฟล์</dt><dd className="cx-num">{d.file_count} ไฟล์ · {d.subfolder_count} โฟลเดอร์ย่อย</dd>
        {pol && <><dt>การควบคุมเวอร์ชัน</dt><dd>{pol.effective.keep_versions === false ? "ไม่เก็บเวอร์ชันเก่า" : "เก็บทุกเวอร์ชัน"}</dd>
          <dt>การเข้ารหัส</dt><dd>{pol.effective.encrypt ? "เข้ารหัส (AES-256)" : "ไฟล์เข้ารหัสตามค่าของระบบ"}</dd></>}
      </dl>}
    </Modal>
  );
}

// ── Move to… (basket › folder picker) ─────────────────────────────────────
export function MoveModal({ baskets, count, excludeFolders = [], onMove, onClose }: {
  baskets: Basket[]; count: string; excludeFolders?: string[];
  onMove: (sys: string, folder: string | null, label: string) => Promise<void>; onClose: () => void;
}) {
  const [target, setTarget] = React.useState<{ sys: string; folder: string | null; label: string } | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  return (
    <Modal title={`Move ${count}`} onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind="primary" disabled={!target || busy} onClick={async () => {
        if (!target) return;
        setBusy(true); setErr(null);
        try { await onMove(target.sys, target.folder, target.label); onClose(); } catch (e) { setErr((e as Error).message); setBusy(false); }
      }}>{target ? `Move to ${target.label}` : "Move"}</Btn>
    </>}>
      <div className="cx-tree" role="tree">
        {baskets.map((b) => <PickBasket key={b.id} b={b} sel={target} setSel={setTarget} exclude={excludeFolders} />)}
      </div>
      {err && <Note kind="error">{err}</Note>}
    </Modal>
  );
}
function PickBasket({ b, sel, setSel, exclude }: { b: Basket; sel: { sys: string; folder: string | null } | null; setSel: (x: { sys: string; folder: string | null; label: string }) => void; exclude: string[] }) {
  const [open, setOpen] = React.useState(false);
  const [kids, setKids] = React.useState<Folder[] | null>(null);
  React.useEffect(() => { if (open && !kids) api<Folder[]>(`/folders?system_id=${b.id}&parent_id=null`).then(setKids).catch(() => setKids([])); }, [open, kids, b.id]);
  return (
    <>
      <button role="treeitem" aria-expanded={open} aria-selected={sel?.sys === b.id && !sel.folder}
        onClick={() => { setSel({ sys: b.id, folder: null, label: b.name }); setOpen(true); }}>
        <span onClick={(e) => { e.stopPropagation(); setOpen((o) => !o); }}>{open ? <I.chevronDown size={12} /> : <I.chevronRight size={12} />}</span>
        <I.basket />{b.name}
      </button>
      {open && kids?.map((f) => <PickFolder key={f.id} f={f} sys={b.id} depth={1} sel={sel} setSel={setSel} exclude={exclude} />)}
    </>
  );
}
function PickFolder({ f, sys, depth, sel, setSel, exclude }: { f: Folder; sys: string; depth: number; sel: { sys: string; folder: string | null } | null; setSel: (x: { sys: string; folder: string | null; label: string }) => void; exclude: string[] }) {
  const [open, setOpen] = React.useState(false);
  const [kids, setKids] = React.useState<Folder[] | null>(null);
  const blocked = exclude.includes(f.id);
  React.useEffect(() => { if (open && !kids) api<Folder[]>(`/folders?system_id=${sys}&parent_id=${f.id}`).then(setKids).catch(() => setKids([])); }, [open, kids, sys, f.id]);
  return (
    <>
      <button role="treeitem" aria-expanded={open} disabled={blocked} aria-selected={sel?.folder === f.id} style={{ paddingLeft: 8 + depth * 16 }}
        onClick={() => setSel({ sys, folder: f.id, label: f.name })}>
        <span onClick={(e) => { e.stopPropagation(); setOpen((o) => !o); }}>{open ? <I.chevronDown size={12} /> : <I.chevronRight size={12} />}</span>
        <I.folder />{f.name}
      </button>
      {open && !blocked && kids?.map((k) => <PickFolder key={k.id} f={k} sys={sys} depth={depth + 1} sel={sel} setSel={setSel} exclude={exclude} />)}
    </>
  );
}

// ── Change status ─────────────────────────────────────────────────────────
export function StatusModal({ fileId, fileName, onClose, onDone }: { fileId: string; fileName: string; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [next, setNext] = React.useState<{ code: string; label: string }[] | null>(null);
  const [to, setTo] = React.useState("");
  const [comment, setComment] = React.useState("");
  const [err, setErr] = React.useState<string | null>(null);
  React.useEffect(() => { api<{ code: string; label: string }[]>(`/files/${fileId}/status`).then(setNext).catch((e) => setErr(e.message)); }, [fileId]);
  return (
    <Modal title="Change status" label={fileName} size="narrow" onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind="primary" disabled={!to} onClick={async () => {
        try {
          const r = await api<{ label: string }>(`/files/${fileId}/status`, { method: "POST", json: { to, comment: comment || undefined } });
          toast({ text: `Status changed to ${r.label}.` }); onDone(); onClose();
        } catch (e) { setErr((e as Error).message); }
      }}>Change status</Btn>
    </>}>
      {next && next.length === 0 && <Note>This file can&apos;t move to another status, or you don&apos;t have the access it needs.</Note>}
      {next && next.length > 0 && <Dropdown label="New status" value={to} placeholder="Choose a status" options={next.map((n) => ({ value: n.code, label: n.label }))} onChange={setTo} />}
      <TextArea label="Comment (optional)" value={comment} onChange={(e) => setComment(e.target.value)} />
      {err && <Note kind="error">{err}</Note>}
    </Modal>
  );
}

// ── Share (people, groups, agencies · Forward · Copy link · public link) ──
const LEVELS = [{ value: "view", label: "View" }, { value: "edit", label: "Edit" }, { value: "manage", label: "Manage" }];

export function ShareModal({ rtype, rid, name, publicLinks, onClose }: {
  rtype: "file" | "folder" | "basket"; rid: string; name: string; publicLinks?: boolean; onClose: () => void;
}) {
  const toast = useToast();
  const [acl, setAcl] = React.useState<AclResponse | null>(null);
  const [rows, setRows] = React.useState<{ principal: string; label: string; level: string; isNew?: boolean }[]>([]);
  const [inherit, setInherit] = React.useState(true);
  const [notify, setNotify] = React.useState(true);
  const [msg, setMsg] = React.useState("");
  const [q, setQ] = React.useState("");
  const [found, setFound] = React.useState<Principal[]>([]);
  const [newLevel, setNewLevel] = React.useState("view");
  const [err, setErr] = React.useState<string | null>(null);
  const [place, setPlace] = React.useState<{ sys: string; folder: string | null } | null>(rtype === "basket" ? { sys: rid, folder: null } : null);
  React.useEffect(() => {
    if (rtype === "folder") api<{ system_id: string }>(`/folders/${rid}`).then((f) => setPlace({ sys: f.system_id, folder: rid })).catch(() => {});
  }, [rtype, rid]);

  React.useEffect(() => {
    api<AclResponse>(`/acl/${rtype}/${rid}`).then((a) => {
      setAcl(a); setInherit(a.inherit);
      setRows(a.entries.map((e) => ({ principal: e.principal, label: e.principal_label, level: e.level_name })));
    }).catch((e) => setErr(e.message));
  }, [rtype, rid]);
  React.useEffect(() => {
    if (!q.trim()) { setFound([]); return; }
    const t = setTimeout(() => api<Principal[]>(`/principals?q=${encodeURIComponent(q.trim())}`).then(setFound).catch(() => setFound([])), 200);
    return () => clearTimeout(t);
  }, [q]);
  const canManage = acl?.my_level === "manage";
  /// Edit = Forward: add people with View, nothing else.
  const canForward = canManage || acl?.my_level === "edit";
  const internal = typeof window !== "undefined"
    ? `${window.location.origin}${BASE_PATH}${rtype === "file" ? `/f/${rid}` : rtype === "folder" ? `/folder/${rid}` : `/b/${rid}`}` : "";

  const copy = async (text: string, what: string) => {
    try { await navigator.clipboard.writeText(text); toast({ text: `${what} copied.` }); }
    catch { toast({ text: "Copy didn't work here. Select the link and copy it.", error: true }); }
  };
  const save = async () => {
    setErr(null);
    try {
      await api(`/acl/${rtype}/${rid}`, { method: "PUT", json: {
        entries: rows.map((r) => ({ principal: r.principal, level: r.level })),
        inherit: rtype === "folder" ? inherit : undefined, notify, message: msg || undefined,
      } });
      toast({ text: `Access to ${name} saved.` }); onClose();
    } catch (e) { setErr((e as Error).message); }
  };

  return (
    <Modal title={`Share ${name}`} onClose={onClose} footer={canForward ? <>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind="primary" onClick={save}>Save</Btn>
    </> : <Btn kind="secondary" onClick={onClose}>Close</Btn>}>
      {canForward && (
        <div style={{ position: "relative" }}>
          <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 120px", gap: 8, alignItems: "end" }}>
            <TextField label="Add people, groups or agencies" placeholder="Name, email or agency code" value={q} onChange={(e) => setQ(e.target.value)} />
            <Dropdown label="Access" value={canManage ? newLevel : "view"} options={canManage ? LEVELS : LEVELS.slice(0, 1)} onChange={setNewLevel} disabled={!canManage} />
          </div>
          {found.length > 0 && (
            <div className="cx-dd-menu" style={{ right: 128 }} role="listbox">
              {found.filter((p) => !rows.some((r) => r.principal === p.principal)).map((p) => (
                <button key={p.principal} className="cx-dd-opt" onClick={() => {
                  setRows((r) => [...r, { principal: p.principal, label: p.label, level: canManage ? newLevel : "view", isNew: true }]); setQ(""); setFound([]);
                }}>{p.label}<small>{p.kind === "agency" ? `Agency ${p.detail ?? ""}` : p.kind === "user" ? p.detail : p.kind === "iamx_role" ? "IAMX role" : p.kind === "role" ? "Role" : ""}</small></button>
              ))}
            </div>
          )}
        </div>
      )}
      <ul className="cx-list" style={{ background: "var(--cx-layer-2)", padding: "0 12px" }}>
        {acl == null && <li><span className="cx-skel" style={{ width: "60%" }} /></li>}
        {rows.map((r, i) => (
          <li key={r.principal}>
            <span className="cx-grow">{r.label}{r.isNew && <> <Tag color="blue">New</Tag></>}</span>
            {canManage || (canForward && r.isNew) ? <>
              <Dropdown className="" value={r.level} disabled={!canManage} options={LEVELS} onChange={(v) => setRows((x) => x.map((y, j) => j === i ? { ...y, level: v } : y))} />
              <button className="cx-close" style={{ width: 28, height: 28 }} aria-label={`Remove ${r.label}`} onClick={() => setRows((x) => x.filter((_, j) => j !== i))}><I.close size={12} /></button>
            </> : <span className="cx-sub">{LEVELS.find((l) => l.value === r.level)?.label}</span>}
          </li>
        ))}
        {acl && inherit && acl.inherited.map((e) => (
          <li key={e.id}>
            <span className="cx-grow">{e.principal_label}<div className="cx-helper">Inherited from {e.inherited_from}</div></span>
            <span className="cx-sub">{LEVELS.find((l) => l.value === e.level_name)?.label}</span>
          </li>
        ))}
        {acl && rows.length === 0 && (!inherit || acl.inherited.length === 0) && <li className="cx-sub">No one has been given access here. Basket roles apply.</li>}
      </ul>
      {canManage && rtype === "folder" && (
        <Check label="Inherit access from the folders above" checked={inherit} onChange={setInherit} />
      )}
      {canForward && rows.some((r) => r.isNew) && (
        <>
          <Check label="Notify people you added (Forward)" checked={notify} onChange={setNotify} />
          {notify && <TextArea label="Message (optional)" value={msg} onChange={(e) => setMsg(e.target.value)} />}
        </>
      )}
      <div style={{ borderTop: "1px solid var(--cx-border-subtle)", paddingTop: 12, display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
        <div style={{ flex: 1, minWidth: 200 }}><div>Link</div><div className="cx-helper">Only people with access can open it.</div></div>
        <Btn kind="tertiary" onClick={() => copy(internal, "Link")}>Copy link</Btn>
      </div>
      {rtype === "folder" && canManage && <FolderEncrypt id={rid} />}
      {rtype !== "file" && canForward && place && <UploadLinks systemId={place.sys} folderId={place.folder} placeName={name} />}
      {rtype === "file" && publicLinks !== false && (acl?.my_level === "edit" || canManage) && <PublicLinks fileId={rid} />}
      {err && <Note kind="error">{err}</Note>}
    </Modal>
  );
}

/// Folder-level encryption switch (TOR 4.15.5).
function FolderEncrypt({ id }: { id: string }) {
  const toast = useToast();
  const [st, setSt] = React.useState<{ own: { encrypt: boolean | null }; effective: { encrypt: boolean | null }; server_has_key: boolean } | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const load = React.useCallback(() => api<typeof st>(`/folders/${id}/policy`).then(setSt).catch(() => {}), [id]);
  React.useEffect(() => { load(); }, [load]);
  if (!st) return null;
  const inherited = st.effective.encrypt && !st.own.encrypt;
  return (
    <div style={{ borderTop: "1px solid var(--cx-border-subtle)", paddingTop: 12, display: "flex", flexDirection: "column", gap: 6 }}>
      <Check label={inherited ? "Encrypt files in this folder (required by the folder or basket above)" : "Encrypt files in this folder"}
        checked={!!st.effective.encrypt} onChange={async (v) => {
          if (inherited || !st.server_has_key) return;
          setErr(null);
          try { const r = await api<{ encrypted_now: number }>(`/folders/${id}/policy`, { method: "PUT", json: { encrypt: v ? true : null } }); toast({ text: v ? `Encryption on. ${r.encrypted_now} file(s) encrypted now.` : "Encryption no longer required here." }); load(); }
          catch (e) { setErr((e as Error).message); }
        }} />
      {!st.server_has_key && <span className="cx-helper">No encryption key is set up on the server.</span>}
      {err && <Note kind="error">{err}</Note>}
    </div>
  );
}
