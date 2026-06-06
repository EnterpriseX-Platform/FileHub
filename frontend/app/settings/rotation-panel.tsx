"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import type { RotationPolicy, RotationRun, System } from "@/lib/api";
import { fmtAgo } from "@/lib/format";

/// Admin-only panel for the rotation engine.  Lists effective policies per
/// scope, lets admins upsert workspace/system policies, and surfaces the
/// last few run results so they can prove rotation is happening (or debug
/// why it isn't).
export function RotationPanel({
  initialPolicies, initialRuns, systems, canMutate,
}: {
  initialPolicies: RotationPolicy[];
  initialRuns: RotationRun[];
  systems: System[];
  canMutate: boolean;
}) {
  const [policies, setPolicies] = React.useState(initialPolicies);
  const [runs, setRuns]         = React.useState(initialRuns);
  const [err, setErr]           = React.useState<string | null>(null);
  const [busy, setBusy]         = React.useState(false);

  const refresh = React.useCallback(async () => {
    const [a, b] = await Promise.all([
      fetch("/filehub/api/rotation/policies", { credentials: "include", cache: "no-store" }),
      fetch("/filehub/api/rotation/runs",     { credentials: "include", cache: "no-store" }),
    ]);
    if (a.ok) setPolicies(await a.json());
    if (b.ok) setRuns(await b.json());
  }, []);

  const upsert = async (p: Omit<RotationPolicy, "id" | "created_at" | "updated_at">) => {
    setErr(null);
    const r = await fetch("/filehub/api/rotation/policies", {
      method: "POST", credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(p),
    });
    if (!r.ok) setErr((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText);
    await refresh();
  };

  const runNow = async () => {
    setBusy(true); setErr(null);
    const r = await fetch("/filehub/api/rotation/run", { method: "POST", credentials: "include" });
    if (!r.ok) setErr((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText);
    await refresh();
    setBusy(false);
  };

  return (
    <div className="card" style={{ padding: 20, marginBottom: 16 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <div>
          <div className="t-md t-semibold">Rotation policies</div>
          <div className="t-xs t-muted">
            Most-specific wins: user → org → system → workspace.  Zero means "no limit at this scope".
          </div>
        </div>
        {canMutate && (
          <button className="btn sm" onClick={runNow} disabled={busy}>
            <Ico.bolt className="icon sm" /> {busy ? "Running…" : "Run now"}
          </button>
        )}
      </div>

      {err && <div className="t-xs" style={{ color: "var(--danger)", marginBottom: 8 }}>{err}</div>}

      <table className="tbl" style={{ marginBottom: 18 }}>
        <thead>
          <tr>
            <th style={{ width: 110 }}>Scope</th>
            <th>Target</th>
            <th style={{ width: 100, textAlign: "right" }}>Keep versions</th>
            <th style={{ width: 130, textAlign: "right" }}>Archive after</th>
            <th style={{ width: 130, textAlign: "right" }}>Hard-delete after</th>
            {canMutate && <th style={{ width: 60 }}></th>}
          </tr>
        </thead>
        <tbody>
          {policies.map((p) => (
            <PolicyRow key={p.id} policy={p} systems={systems} canMutate={canMutate} onChanged={refresh} onError={setErr} />
          ))}
          {canMutate && <PolicyCreateRow systems={systems} onSubmit={upsert} />}
        </tbody>
      </table>

      <div className="t-md t-semibold" style={{ marginBottom: 8 }}>Recent runs</div>
      {runs.length === 0 ? (
        <div className="t-sm t-subtle">No rotation runs yet — press "Run now" to fire one.</div>
      ) : (
        <table className="tbl">
          <thead>
            <tr>
              <th style={{ width: 140 }}>Started</th>
              <th style={{ width: 90 }}>Trigger</th>
              <th style={{ width: 90, textAlign: "right" }}>Pruned</th>
              <th style={{ width: 90, textAlign: "right" }}>Archived</th>
              <th style={{ width: 110, textAlign: "right" }}>Hard-deleted</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {runs.slice(0, 10).map((r) => (
              <tr key={r.id}>
                <td className="t-sm t-muted">{fmtAgo(r.started_at)}</td>
                <td className="t-xs t-mono">{r.triggered_by ?? "—"}</td>
                <td className="t-sm t-tabular" style={{ textAlign: "right" }}>{r.versions_pruned}</td>
                <td className="t-sm t-tabular" style={{ textAlign: "right" }}>{r.files_archived}</td>
                <td className="t-sm t-tabular" style={{ textAlign: "right" }}>{r.files_hard_deleted}</td>
                <td>
                  {r.error
                    ? <Pill tone="rose" sm>{r.error.slice(0, 40)}</Pill>
                    : r.finished_at
                      ? <Pill tone="emerald" sm><span className="dot" />done</Pill>
                      : <Pill tone="amber" sm><span className="dot" />running</Pill>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function PolicyRow({
  policy, systems, canMutate, onChanged, onError,
}: {
  policy: RotationPolicy; systems: System[]; canMutate: boolean;
  onChanged: () => Promise<void>; onError: (m: string | null) => void;
}) {
  const [editing, setEditing] = React.useState(false);
  const [keep, setKeep] = React.useState(String(policy.keep_last_n_versions));
  const [arch, setArch] = React.useState(String(policy.archive_after_days));
  const [del,  setDel]  = React.useState(String(policy.delete_after_days));

  const save = async () => {
    onError(null);
    const r = await fetch("/filehub/api/rotation/policies", {
      method: "POST", credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        scope_type: policy.scope_type, scope_id: policy.scope_id,
        keep_last_n_versions: Number(keep) || 0,
        archive_after_days:   Number(arch) || 0,
        delete_after_days:    Number(del)  || 0,
      }),
    });
    if (!r.ok) { onError((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText); return; }
    setEditing(false);
    await onChanged();
  };

  const remove = async () => {
    onError(null);
    if (policy.scope_type === "workspace") {
      onError("The workspace policy is the fallback — edit it, don't delete it.");
      return;
    }
    if (!confirm("Delete this rotation policy?")) return;
    const r = await fetch(`/filehub/api/rotation/policies/${encodeURIComponent(policy.id)}`, {
      method: "DELETE", credentials: "include",
    });
    if (!r.ok) { onError((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText); return; }
    await onChanged();
  };

  const target = policy.scope_type === "workspace"
    ? "— (default)"
    : policy.scope_type === "system"
      ? (systems.find((s) => s.id === policy.scope_id)?.name ?? policy.scope_id ?? "?")
      : (policy.scope_id ?? "?");

  return (
    <tr>
      <td><Pill sm>{policy.scope_type}</Pill></td>
      <td className="t-sm t-mono t-muted">{target}</td>
      <td className="t-sm t-tabular" style={{ textAlign: "right" }}>
        {editing ? <input className="field" value={keep} onChange={(e) => setKeep(e.target.value)} style={{ width: 70 }} /> : (policy.keep_last_n_versions || "—")}
      </td>
      <td className="t-sm t-tabular" style={{ textAlign: "right" }}>
        {editing ? <input className="field" value={arch} onChange={(e) => setArch(e.target.value)} style={{ width: 70 }} /> : (policy.archive_after_days ? `${policy.archive_after_days} d` : "—")}
      </td>
      <td className="t-sm t-tabular" style={{ textAlign: "right" }}>
        {editing ? <input className="field" value={del} onChange={(e) => setDel(e.target.value)} style={{ width: 70 }} /> : (policy.delete_after_days ? `${policy.delete_after_days} d` : "—")}
      </td>
      {canMutate && (
        <td>
          {editing
            ? <div style={{ display: "flex", gap: 4 }}>
                <button className="btn xs primary" onClick={save}>Save</button>
                <button className="btn xs ghost"   onClick={() => setEditing(false)}>Cancel</button>
              </div>
            : <div style={{ display: "flex", gap: 4 }}>
                <button className="btn xs ghost" onClick={() => setEditing(true)}>Edit</button>
                {policy.scope_type !== "workspace" && (
                  <button className="btn xs ghost" onClick={remove} style={{ color: "var(--danger)" }}>×</button>
                )}
              </div>}
        </td>
      )}
    </tr>
  );
}

function PolicyCreateRow({
  systems, onSubmit,
}: {
  systems: System[];
  onSubmit: (p: Omit<RotationPolicy, "id" | "created_at" | "updated_at">) => Promise<void>;
}) {
  const [scopeType, setScopeType] = React.useState<"system" | "org" | "user">("system");
  const [scopeId, setScopeId] = React.useState("");
  const [keep, setKeep] = React.useState("0");
  const [arch, setArch] = React.useState("0");
  const [del,  setDel]  = React.useState("0");

  return (
    <tr>
      <td>
        <select className="field" value={scopeType} onChange={(e) => setScopeType(e.target.value as "system" | "org" | "user")} style={{ width: "100%" }}>
          <option value="system">system</option>
          <option value="org">org</option>
          <option value="user">user</option>
        </select>
      </td>
      <td>
        {scopeType === "system"
          ? <select className="field" value={scopeId} onChange={(e) => setScopeId(e.target.value)} style={{ width: "100%" }}>
              <option value="">— pick a system —</option>
              {systems.filter((s) => s.system_type === "shared").map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
            </select>
          : <input className="field" value={scopeId} onChange={(e) => setScopeId(e.target.value)} placeholder={`${scopeType} id`} style={{ width: "100%" }} />}
      </td>
      <td><input className="field" value={keep} onChange={(e) => setKeep(e.target.value)} style={{ width: 70, textAlign: "right" }} /></td>
      <td><input className="field" value={arch} onChange={(e) => setArch(e.target.value)} style={{ width: 70, textAlign: "right" }} /></td>
      <td><input className="field" value={del}  onChange={(e) => setDel(e.target.value)}  style={{ width: 70, textAlign: "right" }} /></td>
      <td>
        <button className="btn xs primary" onClick={() => {
          if (!scopeId.trim()) return;
          void onSubmit({
            scope_type: scopeType,
            scope_id: scopeId.trim(),
            keep_last_n_versions: Number(keep) || 0,
            archive_after_days:   Number(arch) || 0,
            delete_after_days:    Number(del)  || 0,
          });
          setScopeId(""); setKeep("0"); setArch("0"); setDel("0");
        }}>Add</button>
      </td>
    </tr>
  );
}
