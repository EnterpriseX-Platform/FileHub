"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";

import { api, bytes, choiceList, when, type Basket, type Category, type FieldDef, type Settings, type StatusDef, type TagColor, type Transition } from "@/lib/fh";
import { useAuth } from "@/lib/auth-context";
import { loadSettings } from "./basket-view";
import { ShareModal } from "./modals";
import { AdminConnectors } from "./connectors";
import { useNav } from "./shell";
import { AdminAgencies, AdminApiKeys, AdminLog, AdminPeople, AdminSettings } from "./admin-tabs";
import { Btn, Check, Dropdown, I, Modal, Note, Tabs, Tag, TextField, useToast } from "./ui";

type TabId = "categories" | "fields" | "statuses" | "policy" | "access" | "folder" | "basket";
const COLORS: { value: TagColor; label: string }[] = [
  { value: "gray", label: "Gray" }, { value: "blue", label: "Blue" }, { value: "green", label: "Green" }, { value: "orange", label: "Orange" }, { value: "red", label: "Red" },
];
const TYPES = [
  { value: "text", label: "Text" }, { value: "number", label: "Number" }, { value: "date", label: "Date" },
  { value: "choice", label: "Choice" }, { value: "user", label: "Person" }, { value: "org", label: "Agency" },
];
const LEVEL = [{ value: "1", label: "View" }, { value: "2", label: "Edit" }, { value: "3", label: "Manage" }];

/// AD-02 Basket settings: Categories · Fields · Statuses · Policy · Access.
export function BasketSettings({ id }: { id: string }) {
  const nav = useNav();
  const { user } = useAuth();
  const toast = useToast();
  const [basket, setBasket] = React.useState<Basket | null>(null);
  const [s, setS] = React.useState<Settings | null>(null);
  const [tab, setTab] = React.useState<TabId>("categories");
  const [err, setErr] = React.useState<string | null>(null);
  const [dirty, setDirty] = React.useState(false);
  const [share, setShare] = React.useState(false);
  const [extText, setExtText] = React.useState("");

  React.useEffect(() => {
    Promise.all([api<Basket[]>("/systems"), api<Settings>(`/baskets/${id}/settings`)]).then(([all, st]) => {
      setBasket(all.find((b) => b.id === id) ?? null); setS(st); setExtText((st.policy.allowed_ext ?? []).join(", "));
    }).catch((e) => setErr(e.message));
  }, [id]);
  const upd = (f: (x: Settings) => Settings) => { setS((x) => (x ? f(x) : x)); setDirty(true); };

  const save = async () => {
    if (!s) return;
    setErr(null);
    const allowed = extText.split(/[,\s]+/).map((x) => x.replace(/^\./, "").trim().toLowerCase()).filter(Boolean);
    try {
      const out = await api<Settings>(`/baskets/${id}/settings`, { method: "PUT", json: { ...s, policy: { ...s.policy, allowed_ext: allowed.length ? allowed : null } } });
      setS(out); setDirty(false); loadSettings(id, true); toast({ text: "Basket settings saved." });
    } catch (e) { setErr((e as Error).message); }
  };

  if (err && !s) return <div className="cx-page"><Note kind="error">{err}</Note></div>;
  return (
    <div className="cx-page" style={{ maxWidth: 1100 }}>
      <div className="cx-crumbs"><Link href="/admin">Admin</Link><span>/</span><Link href={`/b/${id}`}>{basket?.name ?? id}</Link></div>
      <div className="cx-title-row">
        <h1 className="cx-title">{basket?.name ?? " "} settings</h1>
        <span className="cx-spacer" />
        {dirty && <span className="cx-sub">Unsaved changes</span>}
        <Btn kind="primary" disabled={!dirty} onClick={save}>Save</Btn>
      </div>
      <div style={{ background: "var(--cx-layer)" }}>
        <Tabs<TabId> value={tab} onChange={setTab} tabs={[
          { id: "categories", label: "Categories" }, { id: "fields", label: "Fields" }, { id: "statuses", label: "Statuses" },
          { id: "policy", label: "Policy" }, { id: "access", label: "Access" },
          ...(user?.role === "admin" && !basket?.vault ? [{ id: "folder" as TabId, label: "Mounted folder" }] : []),
        ]} />
        {err && <div style={{ padding: 12 }}><Note kind="error">{err}</Note></div>}
        {!s && <div style={{ padding: 16 }}><span className="cx-skel" style={{ width: "40%" }} /></div>}
        {s && tab === "categories" && <Categories list={s.categories} onChange={(c) => upd((x) => ({ ...x, categories: c }))} />}
        {s && tab === "fields" && <Fields list={s.fields} onChange={(f) => upd((x) => ({ ...x, fields: f }))} />}
        {s && tab === "statuses" && <Statuses list={s.statuses} transitions={s.transitions}
          onChange={(st, tr) => upd((x) => ({ ...x, statuses: st, transitions: tr }))} />}
        {s && tab === "policy" && (
          <div className="cx-on-layer" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 16, maxWidth: 640 }}>
            <TextField label="Allowed file types"
              value={extText} onChange={(e) => { setExtText(e.target.value); setDirty(true); }} placeholder="pdf, docx, xlsx, pptx, png, jpg, wav" />
            <TextField label="Largest file (MB)" inputMode="numeric" placeholder="Workspace limit"
              value={s.policy.max_file_bytes ? String(Math.round(s.policy.max_file_bytes / 1048576)) : ""}
              onChange={(e) => { const n = parseInt(e.target.value, 10); upd((x) => ({ ...x, policy: { ...x.policy, max_file_bytes: Number.isFinite(n) && n > 0 ? n * 1048576 : null } })); }} />
            {basket?.vault ? <Note>Vault: files are always encrypted, downloads are stamped and public links are off.</Note> : <>
            <Check label="Stamp downloads (PDF and Office files download as PDF with who, when and a copy code on every page)" checked={!!s.policy.stamp_downloads} onChange={(v) => upd((x) => ({ ...x, policy: { ...x.policy, stamp_downloads: v } }))} />
            <Check label="Encrypt files stored in this basket" checked={!!s.policy.encrypt} onChange={(v) => upd((x) => ({ ...x, policy: { ...x.policy, encrypt: v } }))} />
            <Check label="Let uploaders choose to encrypt individual files" checked={!!s.policy.file_encrypt_choice}
              onChange={(v) => upd((x) => ({ ...x, policy: { ...x.policy, file_encrypt_choice: v, file_encrypt_mode: v ? (x.policy.file_encrypt_mode ?? "symmetric") : x.policy.file_encrypt_mode } }))} />
            {s.policy.file_encrypt_choice && (
              <div style={{ maxWidth: 360, paddingLeft: 28 }}>
                <Dropdown label="Encrypt selected files with" value={s.policy.file_encrypt_mode ?? "symmetric"}
                  options={[{ value: "symmetric", label: "Symmetric (AES-256)", hint: "Server key" }, { value: "asymmetric", label: "Asymmetric (RSA)", hint: "Per-file key, wrapped with RSA" }]}
                  onChange={(v) => upd((x) => ({ ...x, policy: { ...x.policy, file_encrypt_mode: v } }))} />
              </div>
            )}
            <Check label="Allow public links (people outside the organization)" checked={s.policy.public_links !== false} onChange={(v) => upd((x) => ({ ...x, policy: { ...x.policy, public_links: v } }))} />
            </>}
            {s.policy.max_file_bytes ? <span className="cx-helper">Current limit {bytes(s.policy.max_file_bytes)}.</span> : null}
          </div>
        )}
        {tab === "folder" && basket && <MountedFolder basket={basket} onChanged={() => { nav.reload(); api<Basket[]>("/systems").then((all) => setBasket(all.find((b) => b.id === id) ?? null)); }} />}
        {s && tab === "access" && (
          <div style={{ padding: 16, display: "flex", flexDirection: "column", gap: 12, alignItems: "flex-start" }}>
            <Btn kind="tertiary" onClick={() => setShare(true)}>Manage basket access</Btn>
          </div>
        )}
      </div>
      {share && basket && <ShareModal rtype="basket" rid={id} name={basket.name} onClose={() => { setShare(false); nav.reload(); }} />}
    </div>
  );
}

function Categories({ list, onChange }: { list: Category[]; onChange: (c: Category[]) => void }) {
  const set = (i: number, p: Partial<Category>) => onChange(list.map((c, j) => (j === i ? { ...c, ...p } : c)));
  return (
    <div className="cx-table-scroll">
      <table className="cx-table cx-edit-table">
        <thead><tr><th>Name</th><th>Code</th><th>Inside</th><th style={{ width: 32 }} /></tr></thead>
        <tbody>
          {list.map((c, i) => (
            <tr key={i}>
              <td><input className="cx-input" aria-label="Category name" value={c.name} onChange={(e) => set(i, { name: e.target.value })} /></td>
              <td><input className="cx-input" aria-label="Category code" value={c.code} onChange={(e) => set(i, { code: e.target.value.toUpperCase().replace(/[^A-Z0-9_-]/g, "_") })} /></td>
              <td><Dropdown value={c.parent_code ?? ""} options={[{ value: "", label: "Top level" }, ...list.filter((x) => x.code !== c.code).map((x) => ({ value: x.code, label: x.name }))]} onChange={(v) => set(i, { parent_code: v || null })} /></td>
              <td><button className="cx-close" aria-label="Remove category" onClick={() => onChange(list.filter((_, j) => j !== i))}><I.close /></button></td>
            </tr>
          ))}
        </tbody>
      </table>
      <div style={{ background: "var(--cx-layer)" }}><Btn kind="ghost" onClick={() => onChange([...list, { code: `CAT_${list.length + 1}`, name: "", parent_code: null }])}>Add category</Btn></div>
    </div>
  );
}

function Fields({ list, onChange }: { list: FieldDef[]; onChange: (f: FieldDef[]) => void }) {
  const set = (i: number, p: Partial<FieldDef>) => onChange(list.map((c, j) => (j === i ? { ...c, ...p } : c)));
  return (
    <div className="cx-table-scroll">
      <table className="cx-table cx-edit-table">
        <thead><tr><th>Label</th><th>Key</th><th>Type</th><th>Choices</th><th>Required</th><th>Show in list</th><th>Searchable</th><th style={{ width: 32 }} /></tr></thead>
        <tbody>
          {list.map((f, i) => (
            <tr key={i}>
              <td><input className="cx-input" aria-label="Field label" value={f.label} onChange={(e) => set(i, { label: e.target.value })} /></td>
              <td><input className="cx-input" aria-label="Field key" value={f.key} onChange={(e) => set(i, { key: e.target.value.toLowerCase().replace(/[^a-z0-9_]/g, "_") })} /></td>
              <td><Dropdown value={f.field_type} options={TYPES} onChange={(v) => set(i, { field_type: v as FieldDef["field_type"] })} /></td>
              <td>{f.field_type === "choice"
                ? <input className="cx-input" aria-label="Choices" placeholder="High, Medium, Low" value={choiceList(f).map((c) => c.label).join(", ")}
                    onChange={(e) => set(i, { choices: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })} />
                : <span className="cx-helper">—</span>}</td>
              <td className="cx-check"><input type="checkbox" aria-label="Required" checked={f.required} onChange={(e) => set(i, { required: e.target.checked })} /></td>
              <td className="cx-check"><input type="checkbox" aria-label="Show in list" checked={f.show_in_list} onChange={(e) => set(i, { show_in_list: e.target.checked })} /></td>
              <td className="cx-check"><input type="checkbox" aria-label="Searchable" checked={f.searchable} onChange={(e) => set(i, { searchable: e.target.checked })} /></td>
              <td><button className="cx-close" aria-label="Remove field" onClick={() => onChange(list.filter((_, j) => j !== i))}><I.close /></button></td>
            </tr>
          ))}
        </tbody>
      </table>
      <div style={{ background: "var(--cx-layer)" }}><Btn kind="ghost" onClick={() => onChange([...list, { key: `field_${list.length + 1}`, label: "", field_type: "text", required: false, choices: [], searchable: true, show_in_list: false }])}>Add field</Btn></div>
    </div>
  );
}

function Statuses({ list, transitions, onChange }: { list: StatusDef[]; transitions: Transition[]; onChange: (s: StatusDef[], t: Transition[]) => void }) {
  const set = (i: number, p: Partial<StatusDef>) => onChange(list.map((c, j) => (j === i ? { ...c, ...p } : p.is_initial ? { ...c, is_initial: false } : c)), transitions);
  const initial = list.find((s) => s.is_initial);
  return (
    <div>
      <div style={{ padding: "12px 16px", display: "flex", gap: 6, alignItems: "center", flexWrap: "wrap" }}>
        {initial && <Tag color={initial.color}>{initial.label}</Tag>}
        {list.filter((s) => !s.is_initial).map((s) => <React.Fragment key={s.code}><I.chevronRight size={12} /><Tag color={s.color}>{s.label}{s.locks_content && <> <I.lock size={10} /></>}</Tag></React.Fragment>)}
      </div>
      <div className="cx-table-scroll">
        <table className="cx-table cx-edit-table">
          <thead><tr><th>Label</th><th>Code</th><th>Color</th><th>Initial</th><th>Locks file</th><th>Awaits review</th><th>Can move to</th><th style={{ width: 32 }} /></tr></thead>
          <tbody>
            {list.map((s, i) => {
              const outs = transitions.filter((t) => t.from_code === s.code);
              return (
                <tr key={i}>
                  <td><input className="cx-input" aria-label="Status label" value={s.label} onChange={(e) => set(i, { label: e.target.value })} /></td>
                  <td><input className="cx-input" aria-label="Status code" value={s.code} onChange={(e) => {
                    const code = e.target.value.replace(/\s+/g, "_");
                    onChange(list.map((c, j) => (j === i ? { ...c, code } : c)), transitions.map((t) => ({ ...t, from_code: t.from_code === s.code ? code : t.from_code, to_code: t.to_code === s.code ? code : t.to_code })));
                  }} /></td>
                  <td><Dropdown value={s.color} options={COLORS} onChange={(v) => set(i, { color: v as TagColor })} /></td>
                  <td className="cx-check"><input type="radio" name="initial" aria-label="Initial status" checked={s.is_initial} onChange={() => set(i, { is_initial: true })} style={{ accentColor: "var(--cx-text-primary)" }} /></td>
                  <td className="cx-check"><input type="checkbox" aria-label="Locks file" checked={s.locks_content} onChange={(e) => set(i, { locks_content: e.target.checked })} /></td>
                  <td className="cx-check"><input type="checkbox" aria-label="Awaits review" checked={!!s.awaits_review} onChange={(e) => set(i, { awaits_review: e.target.checked })} /></td>
                  <td style={{ whiteSpace: "normal" }}>
                    <div className="cx-chips">
                      {outs.map((t) => {
                        const to = list.find((x) => x.code === t.to_code);
                        return (
                          <span key={t.to_code} style={{ display: "inline-flex", alignItems: "center", gap: 2 }}>
                            <Tag color={to?.color ?? "gray"} onRemove={() => onChange(list, transitions.filter((x) => !(x.from_code === s.code && x.to_code === t.to_code)))}>
                              {to?.label ?? t.to_code} · {LEVEL.find((l) => l.value === String(t.required_level))?.label}
                            </Tag>
                          </span>
                        );
                      })}
                      <Dropdown value="" placeholder="Add" options={list.filter((x) => x.code !== s.code && !outs.some((o) => o.to_code === x.code)).flatMap((x) =>
                        LEVEL.map((l) => ({ value: `${x.code}|${l.value}`, label: `${x.label} (${l.label})` })))}
                        onChange={(v) => { const [to, lvl] = v.split("|"); onChange(list, [...transitions, { from_code: s.code, to_code: to, required_level: Number(lvl) }]); }} />
                    </div>
                  </td>
                  <td><button className="cx-close" aria-label="Remove status" onClick={() => onChange(list.filter((_, j) => j !== i), transitions.filter((t) => t.from_code !== s.code && t.to_code !== s.code))}><I.close /></button></td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <div style={{ background: "var(--cx-layer)" }}><Btn kind="ghost" onClick={() => onChange([...list, { code: `S${list.length + 1}`, label: "", color: "gray", is_initial: list.length === 0, locks_content: false }], transitions)}>Add status</Btn></div>
      </div>
    </div>
  );
}

/// AD-01 Baskets list; "Create basket" opens the form in a dialog.
function AdminBaskets() {
  const nav = useNav();
  const router = useRouter();
  const params = useSearchParams();
  const [creating, setCreating] = React.useState(params.get("new") === "1");
  const { user } = useAuth();
  const isAdmin = user?.role === "admin";
  const [deleting, setDeleting] = React.useState<Basket | null>(null);
  return (
    <>
      <div className="cx-toolbar"><span className="cx-spacer" /><Btn kind="primary" onClick={() => setCreating(true)}>Create basket</Btn></div>
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>Basket</th><th>Storage key</th><th>Status</th><th className="cx-right">Quota</th><th /></tr></thead>
          <tbody>
            {nav.baskets.map((b) => (
              <tr key={b.id}>
                <td className="cx-name"><Link className="cx-filelink" href={`/b/${b.id}`}>{b.vault ? <I.lock /> : <I.basket />}<span>{b.name}</span></Link>{b.vault && <> <Tag color="blue">Vault</Tag></>}</td>
                <td className="cx-num">{b.bucket}</td>
                <td><Tag color={b.status === "live" ? "green" : "gray"}>{b.status === "live" ? "Accepting files" : "Paused"}</Tag></td>
                <td className="cx-right cx-num">{b.quota_bytes ? bytes(b.quota_bytes) : "Workspace"}</td>
                <td className="cx-right"><Link className="cx-btn cx-btn-ghost" href={`/admin/baskets/${b.id}`}>Settings</Link>
                  {isAdmin && <Btn kind="danger-ghost" onClick={() => setDeleting(b)}>Delete</Btn>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {creating && <NewBasketModal onClose={() => { setCreating(false); if (params.get("new")) router.replace("/admin?tab=baskets"); }}
        onCreated={(id) => { nav.reload(); router.push(`/admin/baskets/${id}`); }} />}
      {deleting && <DeleteBasketModal basket={deleting} onClose={() => setDeleting(null)} onDeleted={() => { setDeleting(null); nav.reload(); }} />}
    </>
  );
}

/// Delete a basket: the admin must type its exact name. Only an empty basket can go (the server refuses
/// one that still holds live files), so no file is ever removed by this.
function DeleteBasketModal({ basket, onClose, onDeleted }: { basket: Basket; onClose: () => void; onDeleted: () => void }) {
  const toast = useToast();
  const [typed, setTyped] = React.useState("");
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const match = typed === basket.name;
  const run = async () => {
    if (!match) return;
    setErr(null); setBusy(true);
    try {
      await api(`/systems/${basket.id}`, { method: "DELETE" });
      toast({ text: `Basket ${basket.name} deleted.` }); onDeleted();
    } catch (e) { setErr((e as Error).message); setBusy(false); }
  };
  return (
    <Modal title="Delete basket" size="narrow" onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind="danger" disabled={!match || busy} onClick={run}>{busy ? "Deleting…" : "Delete basket"}</Btn>
    </>}>
      <p>Only an empty basket can be deleted. Move or delete its files first. A deleted basket cannot be restored from this screen.</p>
      <TextField id="del-basket-name" label={`Type ${basket.name} to confirm`} value={typed} autoComplete="off"
        onChange={(e) => setTyped(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") run(); }} />
      {err && <Note kind="error">{err}</Note>}
    </Modal>
  );
}

function NewBasketModal({ onClose, onCreated }: { onClose: () => void; onCreated: (id: string) => void }) {
  const toast = useToast();
  const [name, setName] = React.useState("");
  const [bucket, setBucket] = React.useState("");
  const [vault, setVault] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const key = bucket || name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  const create = async () => {
    setErr(null); setBusy(true);
    try {
      const r = await api<{ id: string }>("/systems", { method: "POST", json: { name: name.trim(), vault, bucket: key, tone: "slate" } });
      toast({ text: `Basket ${name.trim()} created.` }); onCreated(r.id);
    } catch (e) { setErr((e as Error).message); setBusy(false); }
  };
  return (
    <Modal title="Create basket" onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind="primary" disabled={!name.trim() || busy} onClick={create}>Create basket</Btn>
    </>}>
      <div className="cx-form-grid">
        <TextField label="Name" required value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        <TextField label="Storage key" value={bucket} placeholder={key || "e.g. hr-documents"}
          onChange={(e) => setBucket(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, "-"))} />
      </div>
      <Check label="Vault: always encrypted, only people you add, no public links, every open is logged" checked={vault} onChange={setVault} />
      {err && <Note kind="error">{err}</Note>}
    </Modal>
  );
}

const ADMIN_TABS = [
  { id: "baskets", label: "Baskets" },
  { id: "people", label: "People" },
  { id: "agencies", label: "Agencies" },
  { id: "keys", label: "Keys" },
  { id: "connectors", label: "Connectors" },
  { id: "log", label: "Activity log" },
  { id: "settings", label: "Settings" },
] as const;
type AdminTab = (typeof ADMIN_TABS)[number]["id"];

export function AdminHome() {
  const router = useRouter();
  const params = useSearchParams();
  const raw = params.get("tab") === "s3" ? "keys" : params.get("tab") ?? "baskets";
  const tab: AdminTab = (ADMIN_TABS.some((t) => t.id === raw) ? raw : "baskets") as AdminTab;
  return (
    <div className="cx-page" style={{ maxWidth: 1600 }}>
      <div className="cx-title-row"><h1 className="cx-title">Admin</h1></div>
      <div className="cx-admin">
      <nav className="cx-admin-nav" aria-label="Admin sections">
        {ADMIN_TABS.map((t) => (
          <button key={t.id} className="cx-admin-nav-item" aria-current={t.id === tab ? "page" : undefined}
            onClick={() => router.replace(`/admin?tab=${t.id}`)}>{t.label}</button>
        ))}
      </nav>
      <div className="cx-admin-body">
      {tab === "baskets" && <AdminBaskets />}
      {tab === "people" && <AdminPeople />}
      {tab === "agencies" && <AdminAgencies />}
      {tab === "keys" && <AdminApiKeys />}
      {tab === "connectors" && <AdminConnectors />}
      {tab === "log" && <AdminLog />}
      {tab === "settings" && <AdminSettings />}
      </div>
      </div>
    </div>
  );
}

/// A basket that mirrors a folder mounted on the server (Docker volume, NFS, SMB).
function MountedFolder({ basket, onChanged }: { basket: Basket; onChanged: () => void }) {
  const toast = useToast();
  const [roots, setRoots] = React.useState<string[] | null>(null);
  const [path, setPath] = React.useState(basket.mount_path ?? "");
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  React.useEffect(() => { api<string[]>("/mount-roots").then(setRoots).catch(() => setRoots([])); }, []);
  const said = (r: { added: number; updated: number; removed: number }) =>
    toast({ text: `${r.added} added · ${r.updated} updated · ${r.removed} removed.` });
  const save = async (p: string | null) => {
    setErr(null); setBusy(true);
    try {
      const r = await api<{ scan: { added: number; updated: number; removed: number } | null }>(`/baskets/${basket.id}/mount`, { method: "PUT", json: { path: p } });
      if (r.scan) said(r.scan); else toast({ text: "The basket no longer follows a folder." });
      if (p === null) setPath("");
      onChanged();
    } catch (e) { setErr((e as Error).message); }
    setBusy(false);
  };
  const scan = async () => {
    setErr(null); setBusy(true);
    try { said(await api(`/baskets/${basket.id}/scan`, { method: "POST" })); onChanged(); }
    catch (e) { setErr((e as Error).message); }
    setBusy(false);
  };
  return (
    <div className="cx-on-layer" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 16, maxWidth: 640 }}>
      {roots && roots.length === 0 && <Note kind="info">MOUNT_ROOTS isn&apos;t set on this server.</Note>}
      <TextField label="Folder on the server" placeholder={roots?.[0] ? `${roots[0]}/…` : "/mnt/…"} value={path}
        onChange={(e) => setPath(e.target.value)} disabled={!roots?.length} />
      {basket.mount_path && <dl className="cx-kv"><dt>Last scan</dt><dd className="cx-num">{when(basket.mount_scanned_at ?? null) || "Never"}</dd></dl>}
      {err && <Note kind="error">{err}</Note>}
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        <Btn kind="primary" disabled={busy || !path.trim() || path.trim() === basket.mount_path} onClick={() => save(path.trim())}>{basket.mount_path ? "Change folder" : "Mount folder"}</Btn>
        {basket.mount_path && <Btn kind="secondary" disabled={busy} onClick={scan}>Scan now</Btn>}
        {basket.mount_path && <Btn kind="danger-ghost" disabled={busy} onClick={() => save(null)}>Stop following</Btn>}
      </div>
    </div>
  );
}
