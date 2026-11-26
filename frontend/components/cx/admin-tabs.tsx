"use client";

import * as React from "react";

import { api, bytes, fromGbField, toGbField, useApi, when } from "@/lib/fh";
import { useNav } from "./shell";
import { Btn, Confirm, Dropdown, Modal, Note, SearchBar, SkeletonRows, Tag, TextField, useFilter, useToast, OverflowMenu } from "./ui";

const ROLE_LABEL: Record<string, string> = { admin: "Admin", editor: "Editor", viewer: "Viewer" };
const ROLE_OPTS = [
  { value: "viewer", label: "Viewer", hint: "Open and download" },
  { value: "editor", label: "Editor", hint: "Upload, edit, share" },
  { value: "admin", label: "Admin", hint: "Everything, incl. this page" },
];

type Member = {
  id: string; email: string; display_name: string; role: string; status: string; quota_bytes: number;
  used_bytes: number; created_at: string; default_org_id: string | null; source: string | null; last_seen_at: string | null;
};
type Org = { id: string; name: string; code: string; tier: string; system_id: string | null; status: string; quota_bytes: number };

function useOrgs(): Org[] {
  return useApi<Org[]>("/orgs").data ?? [];
}

// ───────────────────────────── People ─────────────────────────────

export function AdminPeople() {
  const toast = useToast();
  const orgs = useOrgs();
  const { data: rows, err, reload: load } = useApi<Member[]>("/users");
  const [q, setQ] = React.useState("");
  const [role, setRole] = React.useState("");
  const [adding, setAdding] = React.useState(false);
  const [editing, setEditing] = React.useState<Member | null>(null);
  const [off, setOff] = React.useState<Member | null>(null);
  const orgName = (id: string | null) => (id ? orgs.find((o) => o.id === id)?.name ?? id : "");
  const matching = useFilter(rows, q, (m) => `${m.display_name} ${m.email} ${orgName(m.default_org_id)}`);
  const shown = matching.filter((m) => !role || m.role === role);
  const patch = async (m: Member, body: Record<string, unknown>, done: string) => {
    try { await api(`/users/${m.id}`, { method: "PATCH", json: body }); toast({ text: done }); load(); }
    catch (e) { toast({ text: (e as Error).message, error: true }); }
  };

  return (
    <>
      <SearchBar value={q} onChange={setQ} placeholder="Search name, email or agency">
        <Dropdown value={role} placeholder="All roles" renderValue={role ? ROLE_LABEL[role] : undefined}
          options={[{ value: "", label: "All roles" }, ...ROLE_OPTS.map(({ value, label }) => ({ value, label }))]} onChange={setRole} />
        <Btn kind="primary" onClick={() => setAdding(true)}>Add person</Btn>
      </SearchBar>
      {err && <Note kind="error">{err}</Note>}
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>Name</th><th className="cx-hide-sm">Email</th><th className="cx-hide-sm">Role</th><th className="cx-hide-sm">Agency</th><th className="cx-hide-sm cx-hide-md">Sign-in</th><th className="cx-hide-sm cx-hide-md">Last seen</th><th>Status</th><th /></tr></thead>
          {rows == null ? <tbody><SkeletonRows cols={8} /></tbody> : (
            <tbody>
              {shown.map((m) => (
                <tr key={m.id}>
                  <td className="cx-name">{m.display_name}</td>
                  <td className="cx-hide-sm">{m.email}</td>
                  <td className="cx-hide-sm">{ROLE_LABEL[m.role] ?? m.role}</td>
                  <td className="cx-hide-sm">{orgName(m.default_org_id) || "—"}</td>
                  <td className="cx-hide-sm cx-hide-md">{m.source === "edge" ? "IAMX" : m.source === "legacy" ? "System" : "Password"}</td>
                  <td className="cx-hide-sm cx-hide-md cx-num">{when(m.last_seen_at) || "—"}</td>
                  <td><Tag color={m.status === "active" ? "green" : m.status === "pending" ? "blue" : "gray"}>{m.status === "active" ? "Active" : m.status === "pending" ? "Waiting for approval" : "Off"}</Tag></td>
                  <td className="cx-right" style={{ width: 48 }}>
                    <OverflowMenu label={`Actions for ${m.display_name}`} items={[
                      { label: "Edit", onClick: () => setEditing(m) },
                      m.status === "active"
                        ? { label: "Turn off", danger: true, onClick: () => setOff(m) }
                        : { label: "Turn on", onClick: () => patch(m, { status: "active" }, `${m.display_name} can sign in again.`) },
                    ]} />
                  </td>
                </tr>
              ))}
            </tbody>
          )}
        </table>
        {rows && shown.length === 0 && <div className="cx-state"><strong>No one matches.</strong></div>}
      </div>
      {adding && <PersonModal orgs={orgs} onClose={() => setAdding(false)} onSaved={() => { setAdding(false); load(); }} />}
      {editing && <PersonModal orgs={orgs} person={editing} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); load(); }} />}
      {off && <Confirm title={`Turn off ${off.display_name}?`} danger action="Turn off"
        body="They are signed out now and can't sign in or use API keys until turned back on. Their files stay where they are."
        onClose={() => setOff(null)} onConfirm={async () => { await patch(off, { status: "disabled" }, `${off.display_name} is turned off.`); setOff(null); }} />}
    </>
  );
}

function PersonModal({ person, orgs, onClose, onSaved }: { person?: Member; orgs: Org[]; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [name, setName] = React.useState(person?.display_name ?? "");
  const [email, setEmail] = React.useState(person?.email ?? "");
  const [pw, setPw] = React.useState("");
  const [role, setRole] = React.useState(person?.role ?? "viewer");
  const [org, setOrg] = React.useState(person?.default_org_id ?? "");
  const [quota, setQuota] = React.useState(toGbField(person?.quota_bytes));
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const save = async () => {
    setErr(null); setBusy(true);
    const quota_bytes = fromGbField(quota);
    try {
      if (person) {
        await api(`/users/${person.id}`, { method: "PATCH", json: { display_name: name, role, default_org_id: org, quota_bytes } });
      } else {
        const r = await api<Member>("/users", { method: "POST", json: { email, display_name: name, password: pw, role, quota_bytes } });
        if (org) await api(`/users/${r.id}`, { method: "PATCH", json: { default_org_id: org } });
      }
      toast({ text: person ? "Saved." : `${name} added.` }); onSaved();
    } catch (e) { setErr((e as Error).message); setBusy(false); }
  };
  return (
    <Modal title={person ? person.display_name : "Add person"} onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind="primary" disabled={busy} onClick={save}>{person ? "Save" : "Add person"}</Btn>
    </>}>
      <div className="cx-form-grid">
        <TextField label="Name" required value={name} onChange={(e) => setName(e.target.value)} />
        <TextField label="Email" required type="email" value={email} disabled={!!person} onChange={(e) => setEmail(e.target.value)} />
        {!person && <TextField label="Password" required type="password" helper="At least 8 characters." value={pw} onChange={(e) => setPw(e.target.value)} />}
        <Dropdown label="Role" value={role} options={ROLE_OPTS} onChange={setRole} />
        <Dropdown label="Agency" value={org} placeholder="None" searchable
          options={[{ value: "", label: "None" }, ...orgs.map((o) => ({ value: o.id, label: o.name, hint: o.code }))]} onChange={setOrg} />
        <TextField label="Storage limit (GB)" placeholder="No limit" inputMode="decimal" value={quota} onChange={(e) => setQuota(e.target.value.replace(/[^0-9.]/g, ""))} />
      </div>
      {person?.source === "edge" && <Note>Signs in with IAMX. Role and agency also follow IAMX at each sign-in.</Note>}
      {err && <Note kind="error">{err}</Note>}
    </Modal>
  );
}

// ───────────────────────────── Agencies ─────────────────────────────

export function AdminAgencies() {
  const nav = useNav();
  const toast = useToast();
  const { data: rows, err, reload: load } = useApi<Org[]>("/orgs");
  const [q, setQ] = React.useState("");
  const [edit, setEdit] = React.useState<Org | null>(null);
  const [gb, setGb] = React.useState("");
  const shown = useFilter(rows, q, (o) => `${o.name} ${o.code}`);
  return (
    <>
      <SearchBar value={q} onChange={setQ} placeholder="Search agency name or code" />
      {err && <Note kind="error">{err}</Note>}
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>Agency</th><th className="cx-hide-sm">Code</th><th className="cx-hide-sm cx-hide-md">Level</th><th className="cx-hide-sm">Basket</th><th className="cx-right">Storage limit</th><th /></tr></thead>
          {rows == null ? <tbody><SkeletonRows cols={6} /></tbody> : (
            <tbody>
              {shown.map((o) => (
                <tr key={o.id}>
                  <td className="cx-name">{o.name}</td>
                  <td className="cx-num cx-hide-sm">{o.code}</td>
                  <td className="cx-hide-sm cx-hide-md">{o.tier}</td>
                  <td className="cx-hide-sm">{nav.baskets.find((b) => b.id === o.system_id)?.name ?? "—"}</td>
                  <td className="cx-right cx-num">{o.quota_bytes ? bytes(o.quota_bytes) : "No limit"}</td>
                  <td className="cx-right"><Btn kind="ghost" onClick={() => { setEdit(o); setGb(toGbField(o.quota_bytes)); }}>Set limit</Btn></td>
                </tr>
              ))}
            </tbody>
          )}
        </table>
        {rows && shown.length === 0 && <div className="cx-state"><strong>No agencies match.</strong></div>}
      </div>
      {edit && (
        <Modal title={`Storage limit — ${edit.name}`} size="narrow" onClose={() => setEdit(null)} footer={<>
          <Btn kind="secondary" onClick={() => setEdit(null)}>Cancel</Btn>
          <Btn kind="primary" onClick={async () => {
            try { await api(`/orgs/${edit.id}/quota`, { method: "PATCH", json: { quota_bytes: fromGbField(gb) } }); toast({ text: "Limit saved." }); setEdit(null); load(); }
            catch (e) { toast({ text: (e as Error).message, error: true }); }
          }}>Save</Btn>
        </>}>
          <TextField label="Limit (GB)" placeholder="No limit" inputMode="decimal" value={gb} onChange={(e) => setGb(e.target.value.replace(/[^0-9.]/g, ""))} />
        </Modal>
      )}
    </>
  );
}

// ───────────────────────────── API keys ─────────────────────────────

type Key = { id: string; name: string; key_prefix: string; user_id: string; last_used_at: string | null; expires_at: string | null; revoked_at: string | null; created_at: string };

type S3Cred = { access_key_id: string; label: string; user_id: string; user_name: string | null; created_at: string; last_used_at: string | null };
type AnyKey = { kind: "api" | "s3"; id: string; name: string; shown: string; user: string; expires: string | null; last: string | null };
type Made = { kind: "api"; key: string } | { kind: "s3"; access_key_id: string; secret_access_key: string };

/// API keys and S3 keys — every credential a system uses to call FileHub, in one list.
export function AdminApiKeys() {
  const toast = useToast();
  const api_ = useApi<Key[]>("/api-keys");
  const ep = useApi<{ enabled: boolean; port: string | null; public_url: string | null }>("/s3-endpoint").data;
  // S3 keys appear only where the server runs the S3 endpoint.
  const s3_ = useApi<S3Cred[]>(ep?.enabled ? "/s3-credentials" : null);
  const members = useApi<Member[]>("/users").data ?? [];
  const [kind, setKind] = React.useState<"api" | "s3">("api");
  const [name, setName] = React.useState("");
  const [uid, setUid] = React.useState("");
  const [days, setDays] = React.useState("365");
  const [made, setMade] = React.useState<Made | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [revoke, setRevoke] = React.useState<AnyKey | null>(null);
  const who = (id: string) => members.find((m) => m.id === id)?.display_name ?? id;
  const loaded = api_.data != null && ep != null && (!ep.enabled || s3_.data != null);
  const rows: AnyKey[] = [
    ...(api_.data ?? []).filter((k) => !k.revoked_at).map((k) => ({ kind: "api" as const, id: k.id, name: k.name, shown: `${k.key_prefix}…`, user: who(k.user_id), expires: k.expires_at, last: k.last_used_at })),
    ...(s3_.data ?? []).map((k) => ({ kind: "s3" as const, id: k.access_key_id, name: k.label, shown: k.access_key_id, user: k.user_name ?? who(k.user_id), expires: null, last: k.last_used_at })),
  ].sort((a, b) => a.name.localeCompare(b.name));
  const endpoint = !ep ? "" : !ep.enabled ? "Not running (S3_PORT)" : ep.public_url || `${window.location.protocol}//${window.location.hostname}:${ep.port}`;
  const copy = async (t: string) => { try { await navigator.clipboard.writeText(t); toast({ text: "Copied." }); } catch { toast({ text: "Copy didn't work here. Select the text and copy it.", error: true }); } };
  const create = async () => {
    setErr(null);
    try {
      if (kind === "api") {
        const r = await api<{ key: string }>("/api-keys", { method: "POST", json: { name, user_id: uid, expires_in_days: Number(days) || undefined } });
        setMade({ kind: "api", key: r.key }); api_.reload();
      } else {
        const r = await api<{ access_key_id: string; secret_access_key: string }>("/s3-credentials", { method: "POST", json: { label: name, user_id: uid || undefined } });
        setMade({ kind: "s3", ...r }); s3_.reload();
      }
      setName("");
    } catch (e) { setErr((e as Error).message); }
  };
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>Name</th>{ep?.enabled && <th>Type</th>}<th>Key</th><th>Acts as</th><th className="cx-hide-sm">Expires</th><th className="cx-hide-sm">Last used</th><th /></tr></thead>
          {!loaded ? <tbody><SkeletonRows cols={7} /></tbody> : (
            <tbody>
              {rows.map((k) => (
                <tr key={`${k.kind}:${k.id}`}>
                  <td className="cx-name">{k.name}</td>
                  {ep?.enabled && <td>{k.kind === "api" ? "API" : "S3"}</td>}
                  <td className="cx-num" style={{ fontFamily: "var(--cx-mono)" }}>{k.shown}</td>
                  <td>{k.user}</td>
                  <td className="cx-hide-sm cx-num">{k.expires ? when(k.expires, false) : "Never"}</td>
                  <td className="cx-hide-sm cx-num">{when(k.last) || "Never"}</td>
                  <td className="cx-right"><Btn kind="danger-ghost" onClick={() => setRevoke(k)}>Revoke</Btn></td>
                </tr>
              ))}
            </tbody>
          )}
        </table>
        {loaded && rows.length === 0 && <div className="cx-state"><strong>No keys.</strong></div>}
      </div>
      <div className="cx-on-layer" style={{ background: "var(--cx-layer)", padding: 16, display: "flex", flexDirection: "column", gap: 12 }}>
        <div style={{ fontWeight: 600 }}>New key</div>
        <div className="cx-form-grid">
          {ep?.enabled && <Dropdown label="Type" value={kind} options={[{ value: "api", label: "API key" }, { value: "s3", label: "S3 key", hint: endpoint }]} onChange={(v) => setKind(v as "api" | "s3")} />}
          <TextField label="Name" placeholder="e.g. Document intake" value={name} onChange={(e) => setName(e.target.value)} />
          <Dropdown label="Acts as" value={uid} placeholder="Choose a person or service account" searchable
            options={members.filter((m) => m.status === "active").map((m) => ({ value: m.id, label: m.display_name, hint: m.email }))} onChange={setUid} />
          {kind === "api" && <Dropdown label="Expires after" value={days} options={[{ value: "30", label: "30 days" }, { value: "90", label: "90 days" }, { value: "365", label: "1 year" }, { value: "0", label: "Never" }]} onChange={setDays} />}
        </div>
        {(err || api_.err || s3_.err) && <Note kind="error">{err || api_.err || s3_.err}</Note>}
        <div><Btn kind="primary" disabled={!name.trim() || !uid} onClick={create}>Create key</Btn></div>
        {made && (
          <Note kind="success">
            <div style={{ display: "flex", flexDirection: "column", gap: 6, flex: 1 }}>
              <div>Copy it now. It won&apos;t be shown again.</div>
              {made.kind === "api" ? (
                <div style={{ fontFamily: "var(--cx-mono)", overflowWrap: "anywhere" }}>{made.key}</div>
              ) : (<>
                <div style={{ fontFamily: "var(--cx-mono)", overflowWrap: "anywhere" }}>Endpoint: {endpoint}</div>
                <div style={{ fontFamily: "var(--cx-mono)", overflowWrap: "anywhere" }}>Access key: {made.access_key_id}</div>
                <div style={{ fontFamily: "var(--cx-mono)", overflowWrap: "anywhere" }}>Secret: {made.secret_access_key}</div>
              </>)}
              <div><Btn kind="tertiary" onClick={() => copy(made.kind === "api" ? made.key : `${made.access_key_id}\n${made.secret_access_key}`)}>Copy</Btn> <Btn kind="ghost" onClick={() => setMade(null)}>Done</Btn></div>
            </div>
          </Note>
        )}
      </div>
      {revoke && <Confirm title={`Revoke ${revoke.name}?`} danger action="Revoke" body="Systems using this key stop working at once."
        onClose={() => setRevoke(null)} onConfirm={async () => {
          try { await api(revoke.kind === "api" ? `/api-keys/${revoke.id}` : `/s3-credentials/${revoke.id}`, { method: "DELETE" }); toast({ text: "Key revoked." }); }
          catch (e) { toast({ text: (e as Error).message, error: true }); }
          setRevoke(null); api_.reload(); s3_.reload();
        }} />}
    </div>
  );
}

// ───────────────────────────── Activity log ─────────────────────────────

type Act = { id: string; actor: string; action: string; target: string; target_type: string | null; system_id: string | null; created_at: string };
const PAGE = 50;

export function AdminLog() {
  const nav = useNav();
  const [rows, setRows] = React.useState<Act[] | null>(null);
  const [q, setQ] = React.useState("");
  const [sys, setSys] = React.useState("");
  const [page, setPage] = React.useState(0);
  React.useEffect(() => {
    setRows(null);
    const t = setTimeout(() => {
      const p = new URLSearchParams({ limit: String(PAGE), offset: String(page * PAGE) });
      if (q.trim()) p.set("q", q.trim());
      if (sys) p.set("system_id", sys);
      api<Act[]>(`/activity?${p}`).then(setRows).catch(() => setRows([]));
    }, 250);
    return () => clearTimeout(t);
  }, [q, sys, page]);
  const basket = (id: string | null) => nav.baskets.find((b) => b.id === id)?.name ?? "";
  return (
    <>
      <SearchBar value={q} onChange={(v) => { setQ(v); setPage(0); }} placeholder="Search person, action or item">
        <Dropdown value={sys} placeholder="All baskets" renderValue={sys ? basket(sys) : undefined}
          options={[{ value: "", label: "All baskets" }, ...nav.baskets.map((b) => ({ value: b.id, label: b.name }))]} onChange={(v) => { setSys(v); setPage(0); }} />
      </SearchBar>
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>When</th><th>Who</th><th>Did</th><th>Item</th><th className="cx-hide-sm cx-hide-md">Basket</th></tr></thead>
          {rows == null ? <tbody><SkeletonRows cols={4} /></tbody> : (
            <tbody>
              {rows.map((a) => (
                <tr key={a.id}>
                  <td className="cx-num">{when(a.created_at)}</td>
                  <td>{a.actor}</td>
                  <td style={{ maxWidth: 220 }} title={a.action}>{a.action}</td>
                  <td className="cx-name" style={{ maxWidth: 200 }} title={a.target}>{a.target}</td>
                  <td className="cx-hide-sm cx-hide-md">{basket(a.system_id)}</td>
                </tr>
              ))}
            </tbody>
          )}
        </table>
        {rows && rows.length === 0 && <div className="cx-state"><strong>Nothing found.</strong></div>}
      </div>
      <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
        <Btn kind="ghost" disabled={page === 0} onClick={() => setPage((p) => p - 1)}>Newer</Btn>
        <Btn kind="ghost" disabled={!rows || rows.length < PAGE} onClick={() => setPage((p) => p + 1)}>Older</Btn>
      </div>
    </>
  );
}

// ───────────────────────────── Settings ─────────────────────────────

export function AdminSettings() {
  const toast = useToast();
  const [v, setV] = React.useState<Record<string, string> | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  // Last values the server confirmed; Save sends only keys that differ from these.
  // Sending every key put collabora_url "http://localhost:9980" in each request,
  // which the edge WAF rejects (403 HTML) even when the admin only renamed the app.
  const saved = React.useRef<Record<string, string>>({});
  React.useEffect(() => { api<Record<string, string>>("/workspace").then((x) => { saved.current = x; setV(x); }).catch((e) => setErr(e.message)); }, []);
  const set = (k: string) => (x: string) => setV((o) => ({ ...(o ?? {}), [k]: x }));
  if (!v) return err ? <Note kind="error">{err}</Note> : <div className="cx-table-scroll"><table className="cx-table"><tbody><SkeletonRows cols={2} rows={4} /></tbody></table></div>;
  const save = async () => {
    setErr(null);
    const keys = ["brand_name", "workspace_display", "org_name", "office_viewer", "collabora_url", "fiscal_year_start_month", "fiscal_year_era", "portal_url", "portal_label", "access_help", "self_signup"];
    const values = Object.fromEntries(keys.filter((k) => (v[k] ?? "") !== (saved.current[k] ?? "")).map((k) => [k, v[k] ?? ""]));
    if (Object.keys(values).length === 0) { toast({ text: "Settings saved." }); return; }
    try { const x = await api<Record<string, string>>("/workspace", { method: "PATCH", json: { values } }); saved.current = x; setV((o) => ({ ...x, ...(o ?? {}) })); toast({ text: "Settings saved." }); }
    catch (e) { setErr((e as Error).message); }
  };
  return (
    <div className="cx-on-layer" style={{ background: "var(--cx-layer)", padding: 16, display: "flex", flexDirection: "column", gap: 16, maxWidth: 760 }}>
      <div style={{ fontWeight: 600 }}>Workspace</div>
      <div className="cx-form-grid">
        <TextField label="Product name" value={v.brand_name ?? ""} onChange={(e) => set("brand_name")(e.target.value)} />
        <TextField label="Workspace name" value={v.workspace_display ?? ""} onChange={(e) => set("workspace_display")(e.target.value)} />
        <TextField label="Organisation" value={v.org_name ?? ""} onChange={(e) => set("org_name")(e.target.value)} />
        <TextField label="Back-to-portal link" value={v.portal_url ?? ""} onChange={(e) => set("portal_url")(e.target.value)} />
        <TextField label="Back-to-portal label" value={v.portal_label ?? ""} onChange={(e) => set("portal_label")(e.target.value)} />
        <TextField className="cx-span" label="Message when someone has no access" value={v.access_help ?? ""} onChange={(e) => set("access_help")(e.target.value)} />
      </div>
      <div style={{ fontWeight: 600 }}>Sign-up</div>
      <div className="cx-form-grid">
        <Dropdown label="People can ask for an account" value={v.self_signup ?? "off"} onChange={set("self_signup")} options={[
          { value: "on", label: "On", hint: "New accounts wait for an admin" },
          { value: "off", label: "Off" },
        ]} />
      </div>
      <div style={{ fontWeight: 600 }}>Opening Office files</div>
      <div className="cx-form-grid">
        <Dropdown label="Word, Excel, PowerPoint" value={v.office_viewer ?? "pdf"} onChange={set("office_viewer")} options={[
          { value: "pdf", label: "View as PDF", hint: "Built in" },
          { value: "collabora", label: "View and edit online", hint: "Needs Collabora Online" },
          { value: "disabled", label: "Download only" },
        ]} />
        {v.office_viewer === "collabora" && <TextField label="Collabora Online address" placeholder="https://office.example.go.th" value={v.collabora_url ?? ""} onChange={(e) => set("collabora_url")(e.target.value)} />}
      </div>
      <div style={{ fontWeight: 600 }}>Fiscal year</div>
      <div className="cx-form-grid">
        <Dropdown label="Year starts in" value={v.fiscal_year_start_month ?? "0"} onChange={set("fiscal_year_start_month")}
          options={["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"].map((m, i) => ({ value: String(i), label: m }))} />
        <Dropdown label="Year shown as" value={v.fiscal_year_era ?? "CE"} onChange={set("fiscal_year_era")} options={[{ value: "CE", label: "2026 (CE)" }, { value: "BE", label: "2569 (BE)" }]} />
      </div>
      {err && <Note kind="error">{err}</Note>}
      <div><Btn kind="primary" onClick={save}>Save settings</Btn></div>
    </div>
  );
}
