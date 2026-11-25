"use client";

import * as React from "react";

import { api, useApi, when } from "@/lib/fh";
import { useNav } from "./shell";
import { Btn, Check, Confirm, Dropdown, Modal, Note, OverflowMenu, SkeletonRows, Tag, TextField, useToast } from "./ui";

type Connector = {
  id: string; name: string; kind: string; enabled: boolean; read_only: boolean; trust_callers: boolean; token_hint: string | null;
  path_prefixes: string[]; system_id: string | null; org_id: string | null; role: string;
  calls: number; last_used_at: string | null; created_at: string;
};

const ROLE_OPTS = [{ value: "editor", label: "Editor" }, { value: "viewer", label: "Viewer" }];

/// Admin → Connectors: the outside apps still calling the old /FileService API.
export function AdminConnectors() {
  const nav = useNav();
  const toast = useToast();
  const { data: rows, err, reload } = useApi<Connector[]>("/admin/connectors");
  const [edit, setEdit] = React.useState<Connector | "new" | null>(null);
  const [del, setDel] = React.useState<Connector | null>(null);
  const basket = (id: string | null) => (id ? nav.baskets.find((b) => b.id === id)?.name ?? id : "");
  const toggle = async (c: Connector) => {
    try { await api(`/admin/connectors/${c.id}`, { method: "PATCH", json: { enabled: !c.enabled } }); reload(); }
    catch (e) { toast({ text: (e as Error).message, error: true }); }
  };
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div className="cx-toolbar"><span className="cx-spacer" /><Btn kind="primary" onClick={() => setEdit("new")}>Add connector</Btn></div>
      {err && <Note kind="error">{err}</Note>}
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>Name</th><th className="cx-hide-md">Basket</th><th className="cx-hide-sm">Path prefixes</th><th className="cx-hide-sm cx-hide-md">Token</th><th className="cx-hide-sm cx-hide-md cx-right">Calls</th><th className="cx-hide-sm cx-hide-md">Last used</th><th>Status</th><th /></tr></thead>
          {rows == null ? <tbody><SkeletonRows cols={8} /></tbody> : (
            <tbody>
              {rows.map((c) => (
                <tr key={c.id}>
                  <td className="cx-name">{c.name}</td>
                  <td className="cx-hide-md">{basket(c.system_id)}</td>
                  <td className="cx-hide-sm" style={{ fontFamily: "var(--cx-mono)" }}>{c.path_prefixes.map((p) => `/${p}`).join("  ")}</td>
                  <td className="cx-hide-sm cx-hide-md cx-num" style={{ fontFamily: "var(--cx-mono)" }}>{c.token_hint ? `…${c.token_hint}` : ""}</td>
                  <td className="cx-hide-sm cx-hide-md cx-right cx-num">{c.calls.toLocaleString()}</td>
                  <td className="cx-hide-sm cx-hide-md cx-num">{when(c.last_used_at) || "Never"}</td>
                  <td>{c.enabled ? (c.read_only ? <Tag color="blue">Read only</Tag> : <Tag color="green">On</Tag>) : <Tag color="gray">Off</Tag>}{c.enabled && c.trust_callers && <> <Tag color="red">No token needed</Tag></>}</td>
                  <td className="cx-right" style={{ width: 48 }}>
                    <OverflowMenu label={`Actions for ${c.name}`} items={[
                      { label: "Edit", onClick: () => setEdit(c) },
                      { label: c.enabled ? "Turn off" : "Turn on", onClick: () => toggle(c) },
                      "sep",
                      { label: "Delete", danger: true, onClick: () => setDel(c) },
                    ]} />
                  </td>
                </tr>
              ))}
            </tbody>
          )}
        </table>
        {rows && rows.length === 0 && <div className="cx-state"><strong>No connectors.</strong></div>}
      </div>
      {edit && <ConnectorModal c={edit === "new" ? null : edit} onClose={() => setEdit(null)} onSaved={() => { setEdit(null); reload(); }} />}
      {del && <Confirm title={`Delete ${del.name}?`} danger action="Delete" body="Calls with its token are refused at once."
        onClose={() => setDel(null)} onConfirm={async () => {
          try { await api(`/admin/connectors/${del.id}`, { method: "DELETE" }); toast({ text: `${del.name} deleted.` }); }
          catch (e) { toast({ text: (e as Error).message, error: true }); }
          setDel(null); reload();
        }} />}
    </div>
  );
}

function ConnectorModal({ c, onClose, onSaved }: { c: Connector | null; onClose: () => void; onSaved: () => void }) {
  const nav = useNav();
  const toast = useToast();
  const [name, setName] = React.useState(c?.name ?? "");
  const [sys, setSys] = React.useState(c?.system_id ?? "");
  const [prefixes, setPrefixes] = React.useState((c?.path_prefixes ?? []).join(", "));
  const [role, setRole] = React.useState(c?.role ?? "editor");
  const [readOnly, setReadOnly] = React.useState(c?.read_only ?? false);
  const [trust, setTrust] = React.useState(c?.trust_callers ?? false);
  const [token, setToken] = React.useState("");
  const [made, setMade] = React.useState<string | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);

  const save = async () => {
    setErr(null); setBusy(true);
    const json = {
      name: name.trim(), system_id: sys, role, read_only: readOnly, trust_callers: trust,
      path_prefixes: prefixes.split(/[,\s]+/).map((p) => p.replace(/^\/+|\/+$/g, "").trim()).filter(Boolean),
      ...(token.trim() ? { token: token.trim() } : {}),
    };
    try {
      await api(c ? `/admin/connectors/${c.id}` : "/admin/connectors", { method: c ? "PATCH" : "POST", json });
      toast({ text: c ? "Connector saved." : `${json.name} added.` }); onSaved();
    } catch (e) { setErr((e as Error).message); }
    setBusy(false);
  };
  const newToken = async () => {
    if (!c) return;
    try { const r = await api<{ token: string }>(`/admin/connectors/${c.id}/token`, { method: "POST" }); setMade(r.token); }
    catch (e) { setErr((e as Error).message); }
  };

  return (
    <Modal title={c ? c.name : "Add connector"} onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind="primary" disabled={!name.trim() || busy} onClick={save}>{c ? "Save" : "Add"}</Btn>
    </>}>
      <div className="cx-form-grid">
        <TextField label="Name" value={name} onChange={(e) => setName(e.target.value)} />
        <Dropdown label="Basket" value={sys} placeholder="Choose a basket" searchable
          options={nav.baskets.map((b) => ({ value: b.id, label: b.name, hint: b.bucket }))} onChange={setSys} />
        <TextField label="Path prefixes" placeholder="hr-portal, e-sign" value={prefixes} onChange={(e) => setPrefixes(e.target.value)} />
        <Dropdown label="Acts as" value={role} options={ROLE_OPTS} onChange={setRole} />
        <TextField label={c?.token_hint ? `Token (now …${c.token_hint})` : "Token"} type="password" autoComplete="off"
          value={token} onChange={(e) => setToken(e.target.value)} />
      </div>
      <div style={{ marginTop: 12, display: "flex", gap: 16, alignItems: "center", flexWrap: "wrap" }}>
        <Check label="Read only" checked={readOnly} onChange={setReadOnly} />
        <Check label="Accept callers without a registered token (inside the cluster, or a token signed by the legacy issuer)" checked={trust} onChange={setTrust} />
        {c && <Btn kind="tertiary" onClick={newToken}>Generate new token</Btn>}
      </div>
      {made && (
        <Note kind="success">
          <div style={{ display: "flex", flexDirection: "column", gap: 6, flex: 1 }}>
            <div>Copy the token now. It won&apos;t be shown again.</div>
            <div style={{ fontFamily: "var(--cx-mono)", overflowWrap: "anywhere" }}>{made}</div>
            <div><Btn kind="tertiary" onClick={async () => { try { await navigator.clipboard.writeText(made); toast({ text: "Copied." }); } catch { toast({ text: "Copy didn't work here. Select the text and copy it.", error: true }); } }}>Copy</Btn></div>
          </div>
        </Note>
      )}
      {err && <Note kind="error">{err}</Note>}
    </Modal>
  );
}
