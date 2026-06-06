"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import type { System } from "@/lib/api";
import { fmtBytes } from "@/lib/format";

/// Admin-only client component that lists shared systems and lets admins
/// create / rename / set quota / delete buckets via the new HTTP endpoints.
/// Server-rendered initial data is passed in; mutations refetch through the
/// public `/filehub/api/systems` proxy so changes show up without a reload.
export function SystemsPanel({ initial, canMutate }: { initial: System[]; canMutate: boolean }) {
  const [rows, setRows] = React.useState<System[]>(initial);
  const [creating, setCreating] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);

  const refresh = React.useCallback(async () => {
    const r = await fetch("/filehub/api/systems", { cache: "no-store", credentials: "include" });
    if (r.ok) setRows(await r.json());
  }, []);

  return (
    <div className="card" style={{ padding: 20, marginBottom: 16 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <div>
          <div className="t-md t-semibold">Storage buckets</div>
          <div className="t-xs t-muted">Workspace systems · each maps to one bucket on the active storage backend</div>
        </div>
        {canMutate && (
          <button className="btn sm primary" onClick={() => setCreating(true)} disabled={creating}>
            <Ico.plus className="icon sm" /> New bucket
          </button>
        )}
      </div>

      {err && <div className="t-xs" style={{ color: "var(--danger)", marginBottom: 8 }}>{err}</div>}

      <table className="tbl">
        <thead>
          <tr>
            <th>Name</th>
            <th style={{ width: 180 }}>Bucket</th>
            <th style={{ width: 110 }}>Quota</th>
            <th style={{ width: 90 }}>Status</th>
            {canMutate && <th style={{ width: 100 }}></th>}
          </tr>
        </thead>
        <tbody>
          {creating && (
            <CreateRow
              onCancel={() => setCreating(false)}
              onCreated={async () => { setCreating(false); await refresh(); }}
              onError={setErr}
            />
          )}
          {rows.map((r) => (
            <SystemRow
              key={r.id}
              row={r}
              canMutate={canMutate}
              onChanged={refresh}
              onError={setErr}
            />
          ))}
          {rows.length === 0 && !creating && (
            <tr><td colSpan={canMutate ? 5 : 4} className="t-sm t-subtle" style={{ textAlign: "center", padding: 24 }}>No buckets yet</td></tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

function SystemRow({ row, canMutate, onChanged, onError }: {
  row: System; canMutate: boolean;
  onChanged: () => Promise<void>; onError: (m: string | null) => void;
}) {
  const [editing, setEditing] = React.useState(false);
  const [name, setName]   = React.useState(row.name);
  const [quota, setQuota] = React.useState(String(row.quota_bytes));

  const save = async () => {
    onError(null);
    const r = await fetch(`/filehub/api/systems/${encodeURIComponent(row.id)}`, {
      method: "PATCH", credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, quota_bytes: Number(quota) || 0 }),
    });
    if (!r.ok) { onError((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText); return; }
    setEditing(false);
    await onChanged();
  };

  const del = async () => {
    onError(null);
    if (!confirm(`Delete bucket "${row.name}"?  This is irreversible.`)) return;
    const r = await fetch(`/filehub/api/systems/${encodeURIComponent(row.id)}`, {
      method: "DELETE", credentials: "include",
    });
    if (!r.ok) { onError((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText); return; }
    await onChanged();
  };

  return (
    <tr>
      <td>
        {editing ? (
          <input className="field" value={name} onChange={(e) => setName(e.target.value)} style={{ width: "100%" }} />
        ) : (
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Pill tone={row.tone}><span className="dot" />{row.name}</Pill>
            {row.description && <span className="t-xs t-subtle">{row.description}</span>}
          </div>
        )}
      </td>
      <td className="t-mono t-sm t-muted">{row.bucket}</td>
      <td>
        {editing ? (
          <input className="field" value={quota} onChange={(e) => setQuota(e.target.value)}
                 style={{ width: 90 }} placeholder="bytes (0=∞)" />
        ) : (
          <span className="t-sm t-tabular">{row.quota_bytes > 0 ? fmtBytes(row.quota_bytes) : "—"}</span>
        )}
      </td>
      <td><Pill tone={row.status === "live" ? "emerald" : "slate"} sm><span className="dot" />{row.status}</Pill></td>
      {canMutate && (
        <td>
          {editing ? (
            <div style={{ display: "flex", gap: 4 }}>
              <button className="btn xs primary" onClick={save}>Save</button>
              <button className="btn xs ghost"   onClick={() => { setEditing(false); setName(row.name); setQuota(String(row.quota_bytes)); }}>Cancel</button>
            </div>
          ) : (
            <div style={{ display: "flex", gap: 4 }}>
              <button className="btn xs ghost" onClick={() => setEditing(true)}>Edit</button>
              <button className="btn xs ghost" onClick={del} style={{ color: "var(--danger)" }}>Delete</button>
            </div>
          )}
        </td>
      )}
    </tr>
  );
}

function CreateRow({ onCancel, onCreated, onError }: {
  onCancel: () => void; onCreated: () => Promise<void>; onError: (m: string | null) => void;
}) {
  const [name, setName]     = React.useState("");
  const [bucket, setBucket] = React.useState("");
  const [quota, setQuota]   = React.useState("0");

  const submit = async () => {
    onError(null);
    if (!name.trim()) { onError("name is required"); return; }
    const r = await fetch("/filehub/api/systems", {
      method: "POST", credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        name: name.trim(),
        bucket: bucket.trim() || undefined,
        quota_bytes: Number(quota) || 0,
      }),
    });
    if (!r.ok) { onError((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText); return; }
    setName(""); setBucket(""); setQuota("0");
    await onCreated();
  };

  return (
    <tr>
      <td><input className="field" value={name}   onChange={(e) => setName(e.target.value)}   placeholder="Display name"   style={{ width: "100%" }} /></td>
      <td><input className="field" value={bucket} onChange={(e) => setBucket(e.target.value)} placeholder="auto from name" style={{ width: "100%" }} /></td>
      <td><input className="field" value={quota}  onChange={(e) => setQuota(e.target.value)}  placeholder="bytes"          style={{ width: 90 }} /></td>
      <td>—</td>
      <td>
        <div style={{ display: "flex", gap: 4 }}>
          <button className="btn xs primary" onClick={submit}>Create</button>
          <button className="btn xs ghost"   onClick={onCancel}>Cancel</button>
        </div>
      </td>
    </tr>
  );
}
