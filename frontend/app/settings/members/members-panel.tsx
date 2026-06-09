"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Av, Pill, type Tone } from "@/components/primitives";
import type { Member } from "@/lib/api";
import { fmtAgo, fmtBytes } from "@/lib/format";

const ROLES = ["admin", "editor", "viewer"] as const;
const STATUSES = ["active", "disabled"] as const;
const TONES: Tone[] = ["indigo", "emerald", "amber", "rose", "violet", "cyan", "fuchsia", "slate"];

/// Members table + invite form.  Each row is an inline edit form for role /
/// status / quota.  Mutations go through PATCH /api/users/:id; invitations
/// go through POST /api/users — the backend gates both behind admin role
/// and refuses to disable the last active admin.
export function MembersPanel({ initial, canMutate }: { initial: Member[]; canMutate: boolean }) {
  const [rows, setRows] = React.useState(initial);
  const [inviting, setInviting] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);

  const refresh = React.useCallback(async () => {
    const r = await fetch("/filehub/api/users", { credentials: "include", cache: "no-store" });
    if (r.ok) setRows(await r.json());
  }, []);

  return (
    <div className="card" style={{ padding: 0 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "12px 16px", borderBottom: "1px solid var(--border)" }}>
        <div className="t-sm t-muted">{rows.length} member{rows.length === 1 ? "" : "s"}</div>
        {canMutate && (
          <button className="btn sm primary" onClick={() => setInviting(true)} disabled={inviting}>
            <Ico.plus className="icon sm" /> Invite member
          </button>
        )}
      </div>
      {err && <div className="t-xs" style={{ color: "var(--danger)", padding: "8px 16px" }}>{err}</div>}

      <div className="table-scroll">
      <table className="tbl">
        <thead>
          <tr>
            <th>Name</th>
            <th style={{ width: 200 }}>Email</th>
            <th style={{ width: 110 }}>Role</th>
            <th style={{ width: 110 }}>Status</th>
            <th style={{ width: 160 }}>Quota</th>
            <th style={{ width: 100 }}>Joined</th>
            {canMutate && <th style={{ width: 90 }}></th>}
          </tr>
        </thead>
        <tbody>
          {inviting && (
            <InviteRow
              onCancel={() => setInviting(false)}
              onCreated={async () => { setInviting(false); await refresh(); }}
              onError={setErr}
              canMutate={canMutate}
            />
          )}
          {rows.map((m) => (
            <MemberRow key={m.id} member={m} canMutate={canMutate} onChanged={refresh} onError={setErr} />
          ))}
          {rows.length === 0 && !inviting && (
            <tr><td colSpan={canMutate ? 7 : 6} className="t-sm t-subtle" style={{ textAlign: "center", padding: 24 }}>No members yet</td></tr>
          )}
        </tbody>
      </table>
      </div>
    </div>
  );
}

function MemberRow({ member: m, canMutate, onChanged, onError }: {
  member: Member; canMutate: boolean;
  onChanged: () => Promise<void>; onError: (m: string | null) => void;
}) {
  const [editing, setEditing] = React.useState(false);
  const [loading, setLoading] = React.useState(false);
  const [role,   setRole]   = React.useState<Member["role"]>(m.role);
  const [status, setStatus] = React.useState<Member["status"]>(m.status);
  const [quota,  setQuota]  = React.useState(String(m.quota_bytes));

  const save = async () => {
    onError(null);
    setLoading(true);
    try {
      const r = await fetch(`/filehub/api/users/${encodeURIComponent(m.id)}`, {
        method: "PATCH", credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ role, status, quota_bytes: Number(quota) || 0 }),
      });
      if (!r.ok) { onError((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText); return; }
      setEditing(false);
      await onChanged();
    } finally {
      setLoading(false);
    }
  };

  const quotaPct = m.quota_bytes > 0 ? Math.min(100, Math.round((m.used_bytes / m.quota_bytes) * 100)) : 0;
  return (
    <tr>
      <td>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Av name={m.display_name} tone={TONES.includes(m.avatar_tone as Tone) ? (m.avatar_tone as Tone) : "slate"} />
          <span className="t-medium">{m.display_name}</span>
        </div>
      </td>
      <td className="t-sm t-muted t-trunc"><span className="t-mono t-sm">{m.email}</span></td>
      <td>
        {editing ? (
          <select className="field" value={role} onChange={(e) => setRole(e.target.value as Member["role"])} style={{ width: "100%" }}>
            {ROLES.map((r) => <option key={r} value={r}>{r}</option>)}
          </select>
        ) : <Pill tone={m.role === "admin" ? "indigo" : m.role === "editor" ? "emerald" : "slate"}>{m.role}</Pill>}
      </td>
      <td>
        {editing ? (
          <select className="field" value={status} onChange={(e) => setStatus(e.target.value as Member["status"])} style={{ width: "100%" }}>
            {STATUSES.map((s) => <option key={s} value={s}>{s}</option>)}
          </select>
        ) : <Pill tone={m.status === "active" ? "emerald" : "rose"} sm><span className="dot" />{m.status}</Pill>}
      </td>
      <td>
        {editing ? (
          <input className="field" value={quota} onChange={(e) => setQuota(e.target.value)} placeholder="bytes (0=∞)" style={{ width: 110 }} />
        ) : m.quota_bytes > 0 ? (
          <div>
            <div className="t-sm t-tabular">{fmtBytes(m.used_bytes)} / {fmtBytes(m.quota_bytes)}</div>
            <div className="prog" style={{ height: 4, marginTop: 2 }}>
              <div className="bar" style={{ width: `${quotaPct}%`, background: quotaPct >= 90 ? "var(--c-rose)" : "var(--c-emerald)" }} />
            </div>
          </div>
        ) : (
          <span className="t-sm t-subtle">{fmtBytes(m.used_bytes)} · no cap</span>
        )}
      </td>
      <td className="t-xs t-muted">{fmtAgo(m.created_at)}</td>
      {canMutate && (
        <td>
          {editing ? (
            <div style={{ display: "flex", gap: 4 }}>
              <button className="btn xs primary" onClick={save} disabled={loading}>{loading ? "Saving…" : "Save"}</button>
              <button className="btn xs ghost"   onClick={() => { setEditing(false); setRole(m.role); setStatus(m.status); setQuota(String(m.quota_bytes)); }} disabled={loading}>Cancel</button>
            </div>
          ) : (
            <button className="btn xs ghost" onClick={() => setEditing(true)}>Edit</button>
          )}
        </td>
      )}
    </tr>
  );
}

function InviteRow({ onCancel, onCreated, onError, canMutate }: {
  onCancel: () => void; onCreated: () => Promise<void>;
  onError: (m: string | null) => void; canMutate: boolean;
}) {
  const [email, setEmail]     = React.useState("");
  const [display, setDisplay] = React.useState("");
  const [pw, setPw]           = React.useState("");
  const [role, setRole]       = React.useState<Member["role"]>("viewer");
  const [quota, setQuota]     = React.useState("0");

  const submit = async () => {
    onError(null);
    if (!email.trim() || !display.trim()) { onError("email + display name required"); return; }
    if (pw.length < 8) { onError("password must be at least 8 characters"); return; }
    const r = await fetch("/filehub/api/users", {
      method: "POST", credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email: email.trim(), display_name: display.trim(), password: pw, role, quota_bytes: Number(quota) || 0 }),
    });
    if (!r.ok) { onError((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText); return; }
    setEmail(""); setDisplay(""); setPw(""); setQuota("0");
    await onCreated();
  };

  return (
    <tr style={{ background: "var(--bg-subtle)" }}>
      <td><input className="field" aria-label="Display name" value={display} onChange={(e) => setDisplay(e.target.value)} placeholder="Display name" style={{ width: "100%" }} /></td>
      <td><input className="field" aria-label="Email address" value={email}   onChange={(e) => setEmail(e.target.value)}   placeholder="user@acme.go.th" style={{ width: "100%" }} /></td>
      <td>
        <select className="field" aria-label="Role" value={role} onChange={(e) => setRole(e.target.value as Member["role"])} style={{ width: "100%" }}>
          {ROLES.map((r) => <option key={r} value={r}>{r}</option>)}
        </select>
      </td>
      <td><Pill tone="emerald" sm><span className="dot" />active</Pill></td>
      <td><input className="field" aria-label="Storage quota in bytes (0 = unlimited)" value={quota} onChange={(e) => setQuota(e.target.value)} placeholder="bytes (0=∞)" style={{ width: 110 }} /></td>
      <td><input className="field" type="password" aria-label="Password (minimum 8 characters)" value={pw} onChange={(e) => setPw(e.target.value)} placeholder="≥ 8 chars" style={{ width: "100%" }} /></td>
      {canMutate && (
        <td>
          <div style={{ display: "flex", gap: 4 }}>
            <button className="btn xs primary" onClick={submit}>Invite</button>
            <button className="btn xs ghost"   onClick={onCancel}>Cancel</button>
          </div>
        </td>
      )}
    </tr>
  );
}
