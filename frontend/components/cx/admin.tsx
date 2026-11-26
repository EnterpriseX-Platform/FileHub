"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";

import { api, bytes, choiceList, fromGbField, toGbField, when, type Basket, type Category, type FieldDef, type Settings, type StatusDef, type TagColor, type Transition } from "@/lib/fh";
import { useAuth } from "@/lib/auth-context";
import { loadSettings } from "./basket-view";
import { AdminConnectors } from "./connectors";
import { useNav } from "./shell";
import { AdminAgencies, AdminApiKeys, AdminLog, AdminPeople, AdminSettings } from "./admin-tabs";
import { basketColor, BASKET_COLORS, Btn, Dropdown, I, Modal, Note, Tag, TextField, Toggle, useToast } from "./ui";

const COLORS: { value: TagColor; label: string }[] = [
  { value: "gray", label: "Gray" }, { value: "blue", label: "Blue" }, { value: "green", label: "Green" }, { value: "orange", label: "Orange" }, { value: "red", label: "Red" },
];
const TYPES = [
  { value: "text", label: "Text" }, { value: "number", label: "Number" }, { value: "date", label: "Date" },
  { value: "choice", label: "Choice" }, { value: "user", label: "Person" }, { value: "org", label: "Agency" },
];
const LEVEL = [{ value: "1", label: "View" }, { value: "2", label: "Edit" }, { value: "3", label: "Manage" }];

/// AD-02 Basket settings, in the basket page's Settings tab: a section menu on
/// the left (Statuses & review flow · Fields · Categories · Policy · Folder
/// layout · General), the section's cards on the right.
type Section = "statuses" | "fields" | "categories" | "policy" | "layout" | "general";
const SECTIONS: { id: Section; label: string }[] = [
  { id: "statuses", label: "Statuses & review flow" }, { id: "fields", label: "Fields" }, { id: "categories", label: "Categories" },
  { id: "policy", label: "Policy" }, { id: "layout", label: "Folder layout" }, { id: "general", label: "General" },
];
export function BasketSettings({ id, embedded }: { id: string; embedded?: boolean }) {
  const nav = useNav();
  const { user } = useAuth();
  const toast = useToast();
  const isAdmin = user?.role === "admin";
  const [basket, setBasket] = React.useState<Basket | null>(null);
  const [s, setS] = React.useState<Settings | null>(null);
  const saved = React.useRef<Settings | null>(null);
  const [sec, setSec] = React.useState<Section>("statuses");
  const [err, setErr] = React.useState<string | null>(null);
  const [dirty, setDirty] = React.useState(false);
  const [extText, setExtText] = React.useState("");

  const loadBasket = React.useCallback(() => api<Basket[]>("/systems").then((all) => setBasket(all.find((b) => b.id === id) ?? null)), [id]);
  React.useEffect(() => {
    Promise.all([loadBasket(), api<Settings>(`/baskets/${id}/settings`)]).then(([, st]) => {
      saved.current = st; setS(st); setExtText((st.policy.allowed_ext ?? []).join(", "));
    }).catch((e) => setErr(e.message));
  }, [id, loadBasket]);
  const upd = (f: (x: Settings) => Settings) => { setS((x) => (x ? f(x) : x)); setDirty(true); };
  const pol = (p: Partial<Settings["policy"]>) => upd((x) => ({ ...x, policy: { ...x.policy, ...p } }));

  const save = async () => {
    if (!s) return;
    setErr(null);
    const allowed = extText.split(/[,\s]+/).map((x) => x.replace(/^\./, "").trim().toLowerCase()).filter(Boolean);
    try {
      const out = await api<Settings>(`/baskets/${id}/settings`, { method: "PUT", json: { ...s, policy: { ...s.policy, allowed_ext: allowed.length ? allowed : null } } });
      saved.current = out; setS(out); setDirty(false); loadSettings(id, true); toast({ text: "Basket settings saved." });
    } catch (e) { setErr((e as Error).message); }
  };
  const discard = () => { if (saved.current) { setS(saved.current); setExtText((saved.current.policy.allowed_ext ?? []).join(", ")); } setDirty(false); setErr(null); };
  const vault = !!basket?.vault;

  if (err && !s) return <div className={embedded ? "" : "cx-page"}><Note kind="error">{err}</Note></div>;
  const body = (
    <div className="cx-sideset">
      <nav className="cx-setnav" aria-label="Settings sections">
        {SECTIONS.map((x) => <button key={x.id} aria-current={sec === x.id ? "page" : undefined} onClick={() => setSec(x.id)}>{x.label}</button>)}
      </nav>
      <div className="cx-cards">
        {!s && <div className="cx-card"><div className="cx-card-b"><span className="cx-skel" style={{ width: "40%" }} /><span className="cx-skel" style={{ width: "70%" }} /></div></div>}
        {s && sec === "statuses" && (
          <div className="cx-card">
            <div className="cx-card-h"><span><b>Statuses &amp; review flow</b></span>
              <Btn kind="tertiary" md icon={<I.add />} onClick={() => upd((x) => ({ ...x, statuses: [...x.statuses, { code: `S${x.statuses.length + 1}`, label: "", color: "gray", is_initial: x.statuses.length === 0, locks_content: false }] }))}>Add status</Btn></div>
            <Opt title="Use statuses in this basket"><Toggle label="Use statuses" on={s.use_statuses !== false} onChange={(v) => upd((x) => ({ ...x, use_statuses: v }))} /></Opt>
            {s.use_statuses !== false && <><Flow list={s.statuses} transitions={s.transitions} />
            <Statuses list={s.statuses} transitions={s.transitions} onChange={(st, tr) => upd((x) => ({ ...x, statuses: st, transitions: tr }))} /></>}
          </div>
        )}
        {s && sec === "fields" && (
          <div className="cx-card">
            <div className="cx-card-h"><span><b>Fields</b></span>
              <Btn kind="tertiary" md icon={<I.add />} onClick={() => upd((x) => ({ ...x, fields: [...x.fields, { key: `field_${x.fields.length + 1}`, label: "", field_type: "text", required: false, choices: [], searchable: true, show_in_list: false }] }))}>Add field</Btn></div>
            <Fields list={s.fields} onChange={(f) => upd((x) => ({ ...x, fields: f }))} />
          </div>
        )}
        {s && sec === "categories" && (
          <div className="cx-card">
            <div className="cx-card-h"><span><b>Categories</b></span>
              <Btn kind="tertiary" md icon={<I.add />} onClick={() => upd((x) => ({ ...x, categories: [...x.categories, { code: `CAT_${x.categories.length + 1}`, name: "", parent_code: null }] }))}>Add category</Btn></div>
            <Categories list={s.categories} onChange={(c) => upd((x) => ({ ...x, categories: c }))} />
          </div>
        )}
        {s && sec === "policy" && (
          <div className="cx-card cx-on-layer">
            <div className="cx-card-h"><span><b>Policy</b></span></div>
            <Opt title="Encrypt files stored in this basket"><Toggle label="Encrypt files" on={vault || !!s.policy.encrypt} disabled={vault} onChange={(v) => pol({ encrypt: v })} /></Opt>
            <Opt title="Uploaders may encrypt single files" side={!vault && s.policy.file_encrypt_choice ? (
              <Dropdown value={s.policy.file_encrypt_mode ?? "symmetric"}
                options={[{ value: "symmetric", label: "Symmetric (AES-256)" }, { value: "asymmetric", label: "Asymmetric (RSA)" }]}
                onChange={(v) => pol({ file_encrypt_mode: v })} />) : undefined}>
              <Toggle label="Per-file encryption" on={!!s.policy.file_encrypt_choice} disabled={vault}
                onChange={(v) => pol({ file_encrypt_choice: v, file_encrypt_mode: v ? (s.policy.file_encrypt_mode ?? "symmetric") : s.policy.file_encrypt_mode })} />
            </Opt>
            <Opt title="Stamp downloads"><Toggle label="Stamp downloads" on={vault || !!s.policy.stamp_downloads} disabled={vault} onChange={(v) => pol({ stamp_downloads: v })} /></Opt>
            <Opt title="Search inside files"><Toggle label="Search inside files" on={s.policy.content_search !== false} onChange={(v) => pol({ content_search: v })} /></Opt>
            <Opt title="Allow public links"><Toggle label="Allow public links" on={!vault && s.policy.public_links !== false} disabled={vault} onChange={(v) => pol({ public_links: v })} /></Opt>
            <Opt title="Allowed file types" side={<TextField aria-label="Allowed file types" value={extText} placeholder="pdf, docx, xlsx" onChange={(e) => { setExtText(e.target.value); setDirty(true); }} />} />
            <Opt title="Largest file (MB)" side={<TextField aria-label="Largest file (MB)" inputMode="numeric" placeholder="Workspace limit"
              value={s.policy.max_file_bytes ? String(Math.round(s.policy.max_file_bytes / 1048576)) : ""}
              onChange={(e) => { const n = parseInt(e.target.value, 10); pol({ max_file_bytes: Number.isFinite(n) && n > 0 ? n * 1048576 : null }); }} />} />
          </div>
        )}
        {sec === "layout" && (
          <>
            <FiscalLayout basketId={id} isAdmin={isAdmin} />
            {isAdmin && !vault && basket && (
              <div className="cx-card"><div className="cx-card-h"><span><b>Mounted folder</b></span></div>
                <MountedFolder basket={basket} onChanged={() => { nav.reload(); loadBasket(); }} /></div>
            )}
          </>
        )}
        {sec === "general" && basket && <General basket={basket} isAdmin={isAdmin} onSaved={() => { nav.reload(); loadBasket(); }} />}
        {err && <Note kind="error">{err}</Note>}
        {s && sec !== "general" && sec !== "layout" && (
          <div className="cx-savebar">
            {dirty && <span className="cx-sub">Unsaved changes</span>}
            <Btn kind="secondary" md disabled={!dirty} onClick={discard}>Discard</Btn>
            <Btn kind="primary" md disabled={!dirty} onClick={save}>Save changes</Btn>
          </div>
        )}
      </div>
    </div>
  );
  if (embedded) return body;
  return (
    <div className="cx-page" style={{ maxWidth: 1200 }}>
      <div className="cx-crumbs"><Link href="/baskets">Baskets</Link><I.chevronRight size={12} /><Link href={`/b/${id}`}>{basket?.name ?? id}</Link></div>
      <div className="cx-ph"><div><h1 className="cx-title">Settings</h1></div></div>
      {body}
    </div>
  );
}

function Opt({ title, children, side }: { title: string; children?: React.ReactNode; side?: React.ReactNode }) {
  return <div className="cx-opt"><div className="cx-opt-tx"><b>{title}</b></div>{side && <div className="cx-opt-side">{side}</div>}{children}</div>;
}

/// The review flow drawn from the transitions: the main path from the initial
/// status first, then every other move on its own line; the level needed sits under each arrow.
function Flow({ list, transitions }: { list: StatusDef[]; transitions: Transition[] }) {
  const byCode = new Map(list.map((x) => [x.code, x]));
  const lvl = (n: number) => LEVEL.find((l) => l.value === String(n))?.label ?? "";
  const used = new Set<string>();
  const key = (t: Transition) => `${t.from_code}>${t.to_code}`;
  const rows: Transition[][] = [];
  const start = list.find((x) => x.is_initial)?.code;
  if (start) {
    const path: Transition[] = [];
    const seen = new Set([start]);
    let cur = start;
    for (let g = 0; g < 32; g++) {
      const next = transitions.find((t) => t.from_code === cur && !seen.has(t.to_code) && byCode.has(t.to_code));
      if (!next) break;
      path.push(next); used.add(key(next)); seen.add(next.to_code); cur = next.to_code;
    }
    if (path.length) rows.push(path);
  }
  for (const t of transitions) {
    if (used.has(key(t))) continue;
    used.add(key(t));
    const last = rows[rows.length - 1];
    if (last && rows.length > 1 && last[last.length - 1].to_code === t.from_code) last.push(t); else rows.push([t]);
  }
  const tag = (code: string, k: string) => { const x = byCode.get(code); return <Tag key={k} color={x?.color ?? "gray"}>{x?.label || code}</Tag>; };
  if (!list.length) return null;
  return (
    <div className="cx-flow" aria-label="Review flow">
      {rows.length === 0 && start && tag(start, "only")}
      {rows.map((r, i) => (
        <React.Fragment key={i}>
          {i > 0 && <span className="cx-flow-br" />}
          {tag(r[0].from_code, `${i}-s`)}
          {r.map((t, j) => (
            <React.Fragment key={j}>
              <span className="cx-flow-arr" title={`Needs ${lvl(t.required_level)}`}><I.move size={14} /><span>{lvl(t.required_level)}</span></span>
              {tag(t.to_code, `${i}-${j}`)}
            </React.Fragment>
          ))}
        </React.Fragment>
      ))}
    </div>
  );
}

/// Fiscal-year folder layout (workspace-wide, one basket). Shown here when this basket has it.
function FiscalLayout({ basketId, isAdmin }: { basketId: string; isAdmin: boolean }) {
  const toast = useToast();
  type L = { enabled: boolean; current_year?: number; settings?: { system_id: string; main_name: string; support_name: string }; sources?: { code: string; folder_name: string; group: string }[] };
  const [l, setL] = React.useState<L | null>(null);
  React.useEffect(() => { if (isAdmin) api<L>("/admin/layout").then(setL).catch(() => setL({ enabled: false })); else setL({ enabled: false }); }, [isAdmin]);
  const mine = l?.enabled && l.settings?.system_id === basketId;
  return (
    <div className="cx-card">
      <div className="cx-card-h"><span><b>Fiscal-year layout</b></span>
        {mine && <Btn kind="tertiary" md onClick={async () => { try { await api("/admin/layout/prepare", { method: "POST" }); toast({ text: "Year folders are ready." }); } catch (e) { toast({ text: (e as Error).message, error: true }); } }}>Create year folders</Btn>}</div>
      {l == null && <div className="cx-card-b"><span className="cx-skel" style={{ width: "50%" }} /></div>}
      {l && !mine && <div className="cx-opt"><div className="cx-opt-tx"><span>Off</span></div></div>}
      {mine && l.settings && (
        <div className="cx-layout-tree">
          <b>{l.current_year}</b>{"\n"}
          {(l.sources ?? []).filter((x) => x.group === "main").length > 0 && <>├─ {l.settings.main_name} / &lt;agency&gt; / &lt;department&gt;{"\n"}</>}
          {(l.sources ?? []).filter((x) => x.group === "main").map((x) => <React.Fragment key={x.code}>│   ├─ {x.folder_name}{"\n"}</React.Fragment>)}
          └─ {l.settings.support_name}{"\n"}
        </div>
      )}
    </div>
  );
}

/// Name, about, color, limit and intake of the basket (admins); delete an empty basket.
function General({ basket, isAdmin, onSaved }: { basket: Basket; isAdmin: boolean; onSaved: () => void }) {
  const toast = useToast();
  const router = useRouter();
  const nav = useNav();
  const [name, setName] = React.useState(basket.name);
  const [about, setAbout] = React.useState(basket.description ?? "");
  const [tone, setTone] = React.useState(basket.tone ?? "purple");
  const [gb, setGb] = React.useState(toGbField(basket.quota_bytes));
  const [live, setLive] = React.useState(basket.status === "live");
  const [err, setErr] = React.useState<string | null>(null);
  const [del, setDel] = React.useState(false);
  const changed = name !== basket.name || about !== (basket.description ?? "") || tone !== (basket.tone ?? "purple") || gb !== toGbField(basket.quota_bytes) || live !== (basket.status === "live");
  const save = async () => {
    setErr(null);
    try {
      await api(`/systems/${basket.id}`, { method: "PATCH", json: { name: name.trim(), description: about, tone, quota_bytes: fromGbField(gb), status: live ? "live" : "paused" } });
      toast({ text: "Basket saved." }); onSaved();
    } catch (e) { setErr((e as Error).message); }
  };
  return (
    <>
      <div className="cx-card cx-on-layer">
        <div className="cx-card-h"><span><b>General</b></span></div>
        <div className="cx-card-b">
          <div className="cx-form-grid">
            <TextField label="Name" value={name} disabled={!isAdmin} onChange={(e) => setName(e.target.value)} />
            <TextField label="Storage key" value={basket.bucket} readOnly className="cx-mono" />
            <TextField label="About" className="cx-span" value={about} disabled={!isAdmin} onChange={(e) => setAbout(e.target.value)} />
            <TextField label="Limit (GB)" inputMode="numeric" placeholder="Workspace limit" value={gb} disabled={!isAdmin} onChange={(e) => setGb(e.target.value.replace(/[^\d.]/g, ""))} />
          </div>
          <div className="cx-field"><span className="cx-label">Color</span><Swatches value={tone} onChange={isAdmin ? setTone : () => {}} /></div>
        </div>
        <Opt title="Accepting files"><Toggle label="Accepting files" on={live} disabled={!isAdmin} onChange={setLive} /></Opt>
      </div>
      {err && <Note kind="error">{err}</Note>}
      {isAdmin && (
        <div className="cx-savebar">
          <Btn kind="danger-ghost" md onClick={() => setDel(true)}>Delete basket</Btn>
          <span className="cx-spacer" style={{ flex: 1 }} />
          <Btn kind="primary" md disabled={!changed || !name.trim()} onClick={save}>Save changes</Btn>
        </div>
      )}
      {del && <DeleteBasketModal basket={basket} onClose={() => setDel(false)} onDeleted={() => { setDel(false); nav.reload(); router.push("/baskets"); }} />}
    </>
  );
}

export function Swatches({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="cx-swatches" role="radiogroup" aria-label="Color">
      {BASKET_COLORS.map((c) => (
        <button key={c.value} type="button" role="radio" aria-checked={basketColor(value) === c.color} aria-label={c.value}
          className="cx-swatch" style={{ background: c.color }} onClick={() => onChange(c.value)} />
      ))}
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
    </div>
  );
}

function Statuses({ list, transitions, onChange }: { list: StatusDef[]; transitions: Transition[]; onChange: (s: StatusDef[], t: Transition[]) => void }) {
  const set = (i: number, p: Partial<StatusDef>) => onChange(list.map((c, j) => (j === i ? { ...c, ...p } : p.is_initial ? { ...c, is_initial: false } : c)), transitions);
  return (
    <div>
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
      <div className="cx-toolbar"><span className="cx-spacer" /><Btn kind="primary" icon={<I.add />} onClick={() => setCreating(true)}>New basket</Btn></div>
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>Basket</th><th>Storage key</th><th>Status</th><th className="cx-right">Quota</th><th /></tr></thead>
          <tbody>
            {nav.baskets.map((b) => (
              <tr key={b.id}>
                <td className="cx-name"><Link className="cx-filelink" href={`/b/${b.id}`}><span className="cx-bdot" style={{ background: basketColor(b.tone) }} /><span>{b.name}</span></Link>{b.vault && <> <Tag color="red">Vault</Tag></>}</td>
                <td className="cx-num">{b.bucket}</td>
                <td><Tag color={b.status === "live" ? "green" : "gray"}>{b.status === "live" ? "Accepting files" : "Paused"}</Tag></td>
                <td className="cx-right cx-num">{b.quota_bytes ? bytes(b.quota_bytes) : "Workspace"}</td>
                <td className="cx-right"><Link className="cx-btn cx-btn-ghost" href={`/b/${b.id}?tab=settings`}>Settings</Link>
                  {isAdmin && <Btn kind="danger-ghost" onClick={() => setDeleting(b)}>Delete</Btn>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {creating && <NewBasketModal onClose={() => { setCreating(false); if (params.get("new")) router.replace("/admin?tab=baskets"); }}
        onCreated={(id) => { nav.reload(); router.push(`/b/${id}`); }} />}
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

type Template = "review" | "plain" | "vault";
const TEMPLATES: { id: Template; label: string; sub: string }[] = [
  { id: "review", label: "Review flow", sub: "Draft → In review → Approved" },
  { id: "plain", label: "Plain", sub: "One status" },
  { id: "vault", label: "Vault", sub: "Encrypted · people you add" },
];
/// Statuses and moves a template sets right after the basket is made.
function templateSettings(t: Template, cur: Settings): Settings {
  if (t === "review") return { ...cur,
    statuses: [
      { code: "Draft", label: "Draft", color: "gray", is_initial: true, locks_content: false, awaits_review: false, sort: 1 },
      { code: "Review", label: "In review", color: "blue", is_initial: false, locks_content: true, awaits_review: true, sort: 2 },
      { code: "Approved", label: "Approved", color: "green", is_initial: false, locks_content: true, awaits_review: false, sort: 3 },
      { code: "Rejected", label: "Rejected", color: "red", is_initial: false, locks_content: false, awaits_review: false, sort: 4 },
    ],
    transitions: [
      { from_code: "Draft", to_code: "Review", required_level: 2 }, { from_code: "Review", to_code: "Approved", required_level: 3 },
      { from_code: "Review", to_code: "Rejected", required_level: 3 }, { from_code: "Rejected", to_code: "Draft", required_level: 2 },
    ] };
  return { ...cur,
    statuses: [{ code: "Stored", label: "Stored", color: "gray", is_initial: true, locks_content: false, awaits_review: false, sort: 1 }],
    transitions: [] };
}

export function NewBasketModal({ onClose, onCreated }: { onClose: () => void; onCreated: (id: string) => void }) {
  const toast = useToast();
  const [name, setName] = React.useState("");
  const [bucket, setBucket] = React.useState("");
  const [tone, setTone] = React.useState("purple");
  const [tpl, setTpl] = React.useState<Template>("review");
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const key = bucket || name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  const create = async () => {
    setErr(null); setBusy(true);
    try {
      const r = await api<{ id: string }>("/systems", { method: "POST", json: { name: name.trim(), vault: tpl === "vault", bucket: key || undefined, tone } });
      try {
        const cur = await api<Settings>(`/baskets/${r.id}/settings`);
        await api(`/baskets/${r.id}/settings`, { method: "PUT", json: templateSettings(tpl, cur) });
      } catch (e) { toast({ text: `Basket created; its statuses weren't set: ${(e as Error).message}`, error: true }); }
      toast({ text: `Basket ${name.trim()} created.` }); onCreated(r.id);
    } catch (e) { setErr((e as Error).message); setBusy(false); }
  };
  return (
    <Modal label="Basket" title="New basket" onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind="primary" disabled={!name.trim() || busy} onClick={create}>{busy ? "Creating…" : "Create basket"}</Btn>
    </>}>
      <TextField label="Name" required value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      <TextField label="Storage key" className="cx-mono" value={bucket} placeholder={key || "hr-documents"}
        onChange={(e) => setBucket(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, "-"))} />
      <div className="cx-field"><span className="cx-label">Color</span><Swatches value={tone} onChange={setTone} /></div>
      <div className="cx-field"><span className="cx-label">Template</span>
        <div className="cx-tpl" role="radiogroup" aria-label="Template">
          {TEMPLATES.map((t) => <button key={t.id} type="button" role="radio" aria-checked={tpl === t.id} onClick={() => setTpl(t.id)}><b>{t.label}</b><span>{t.sub}</span></button>)}
        </div>
      </div>
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
