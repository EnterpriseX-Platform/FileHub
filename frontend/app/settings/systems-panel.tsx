"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { mutate } from "@/lib/mutate";
import type { System } from "@/lib/api";
import type { Tone } from "@/components/primitives";
import { fmtBytes, fmtCount } from "@/lib/format";

/// Buckets — typically one per business system or department.
///
/// Previously a table where the quota was typed in raw bytes: "10GB" became
/// `Number("10GB")` = NaN → saved as 0 = unlimited, silently.  Now one card per
/// bucket with usage vs quota and file count; quota is entered in MB/GB/TB.

type Usage = { files: number; bytes: number };

const TONES: { v: Tone; label: string }[] = [
  { v: "indigo", label: "Indigo" }, { v: "emerald", label: "Green" }, { v: "amber", label: "Amber" },
  { v: "rose", label: "Rose" }, { v: "violet", label: "Violet" }, { v: "cyan", label: "Cyan" }, { v: "slate", label: "Slate" },
] as { v: Tone; label: string }[];

const UNITS = [
  { v: "MB", mul: 1024 ** 2 },
  { v: "GB", mul: 1024 ** 3 },
  { v: "TB", mul: 1024 ** 4 },
] as const;
type Unit = (typeof UNITS)[number]["v"];

const BUCKET_RE = /^[a-z0-9][a-z0-9-]{1,62}$/;

function splitQuota(bytes: number): { amount: string; unit: Unit; unlimited: boolean } {
  if (!bytes || bytes <= 0) return { amount: "", unit: "GB", unlimited: true };
  for (const u of [...UNITS].reverse()) {
    if (bytes >= u.mul && bytes % u.mul === 0) return { amount: String(bytes / u.mul), unit: u.v, unlimited: false };
  }
  return { amount: String(Math.round((bytes / 1024 ** 3) * 100) / 100), unit: "GB", unlimited: false };
}

function joinQuota(amount: string, unit: Unit, unlimited: boolean): number | null {
  if (unlimited) return 0;
  const n = Number(amount.replace(/,/g, ""));
  if (!Number.isFinite(n) || n <= 0) return null;
  return Math.round(n * UNITS.find((u) => u.v === unit)!.mul);
}

/// Bucket id suggested from the display name (a-z 0-9 - only; non-Latin names need a manual id).
function suggestBucket(name: string): string {
  return name.toLowerCase().normalize("NFKD").replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 63);
}

export function SystemsPanel({ initial, canMutate }: { initial: System[]; canMutate: boolean }) {
  const [rows, setRows] = React.useState<System[]>(initial);
  const [usage, setUsage] = React.useState<Record<string, Usage>>({});
  const [creating, setCreating] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);

  const refresh = React.useCallback(async () => {
    const [r, st] = await Promise.all([
      fetch("/filehub/api/systems", { cache: "no-store", credentials: "include" }),
      fetch("/filehub/api/stats", { cache: "no-store", credentials: "include" }),
    ]);
    if (r.ok) setRows(((await r.json()) as System[]).filter((s) => s.system_type !== "personal"));
    if (st.ok) {
      const j = await st.json();
      const u: Record<string, Usage> = {};
      for (const s of j.storage_by_system ?? []) u[s.system_id] = { files: s.file_count, bytes: s.size_bytes };
      setUsage(u);
    }
  }, []);

  React.useEffect(() => { void refresh(); }, [refresh]);

  const sorted = [...rows].sort((a, b) => a.name.localeCompare(b.name));

  return (
    <div id="buckets" className="card" style={{ padding: 20, marginBottom: 16, scrollMarginTop: 16 }}>
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 12, marginBottom: 14 }}>
        <div>
          <div className="t-md t-semibold">Buckets</div>
          <div className="t-xs t-muted" style={{ marginTop: 2 }}>
            One bucket per business system — set a storage quota per bucket and always know where a file came from.
          </div>
        </div>
        {canMutate && !creating && (
          <button className="btn sm primary" onClick={() => { setErr(null); setCreating(true); }}>
            <Ico.plus className="icon sm" /> New bucket
          </button>
        )}
      </div>

      {err && (
        <div className="t-sm" role="alert" style={{ color: "var(--danger)", background: "var(--danger-bg, #fef2f2)", padding: "8px 10px", borderRadius: 8, marginBottom: 10 }}>
          {err}
        </div>
      )}

      {creating && (
        <BucketForm
          mode="create"
          existing={rows}
          onCancel={() => setCreating(false)}
          onDone={async () => { setCreating(false); await refresh(); }}
          onError={setErr}
        />
      )}

      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))", gap: 12 }}>
        {sorted.map((r) => (
          <BucketCard key={r.id} row={r} usage={usage[r.id]} canMutate={canMutate} existing={rows}
                      onChanged={refresh} onError={setErr} />
        ))}
      </div>

      {sorted.length === 0 && !creating && (
        <div className="t-sm t-subtle" style={{ textAlign: "center", padding: 24 }}>
          No buckets yet{canMutate ? " — click “New bucket” to create one." : "."}
        </div>
      )}
    </div>
  );
}

function BucketCard({ row, usage, canMutate, existing, onChanged, onError }: {
  row: System; usage?: Usage; canMutate: boolean; existing: System[];
  onChanged: () => Promise<void>; onError: (m: string | null) => void;
}) {
  const [editing, setEditing] = React.useState(false);
  const used = usage?.bytes ?? 0;
  const files = usage?.files ?? 0;
  const quota = row.quota_bytes ?? 0;
  const pct = quota > 0 ? Math.min(100, (used / quota) * 100) : 0;
  const barColor = pct >= 90 ? "var(--danger)" : pct >= 75 ? "var(--warning, #d97706)" : "var(--accent)";

  const del = async () => {
    onError(null);
    if (!confirm(`Delete bucket "${row.name}"?\n\nIt disappears from the list and the id "${row.bucket}" cannot be reused.`)) return;
    const r = await mutate(`/filehub/api/systems/${encodeURIComponent(row.id)}`, "DELETE", { credentials: "include" });
    if (!r.ok) { onError((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText); return; }
    await onChanged();
  };

  if (editing) {
    return (
      <div style={{ gridColumn: "1 / -1" }}>
        <BucketForm mode="edit" row={row} existing={existing}
                    onCancel={() => setEditing(false)}
                    onDone={async () => { setEditing(false); await onChanged(); }}
                    onError={onError} />
      </div>
    );
  }

  return (
    <div className="card" style={{ padding: 14, display: "flex", flexDirection: "column", gap: 10, boxShadow: "none" }}>
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 8 }}>
        <div style={{ minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Pill tone={row.tone} sm><span className="dot" /></Pill>
            <span className="t-semibold" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{row.name}</span>
          </div>
          {row.description && <div className="t-xs t-muted" style={{ marginTop: 4 }}>{row.description}</div>}
        </div>
        {row.status !== "live" && <Pill tone="slate" sm>Paused</Pill>}
      </div>

      <div>
        <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12, marginBottom: 4 }}>
          <span className="t-tabular"><b>{fmtBytes(used)}</b>{quota > 0 ? ` of ${fmtBytes(quota)}` : ""}</span>
          <span className="t-muted">{quota > 0 ? `${pct.toFixed(0)}%` : "No quota"}</span>
        </div>
        <div style={{ height: 6, borderRadius: 999, background: "var(--bg-muted)", overflow: "hidden" }}>
          <div style={{ width: `${quota > 0 ? pct : 0}%`, height: "100%", background: barColor }} />
        </div>
      </div>

      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", fontSize: 12 }}>
        <span className="t-muted">
          {files > 0 ? `${fmtCount(files)} file${files === 1 ? "" : "s"}` : "Empty"} · <span className="t-mono" title="Bucket id in storage">{row.bucket}</span>
        </span>
        <a className="t-xs" href={`/filehub/explorer?b=${encodeURIComponent(row.id)}`} style={{ color: "var(--accent)" }}>Open</a>
      </div>

      {canMutate && (
        <div style={{ display: "flex", gap: 6, borderTop: "1px solid var(--border)", paddingTop: 10 }}>
          <button className="btn xs ghost" onClick={() => { onError(null); setEditing(true); }}>Edit</button>
          <button className="btn xs ghost" onClick={del} disabled={files > 0}
                  title={files > 0 ? `Cannot delete — ${fmtCount(files)} file(s) still in this bucket` : "Delete this bucket"}
                  style={{ color: files > 0 ? undefined : "var(--danger)" }}>
            Delete
          </button>
        </div>
      )}
    </div>
  );
}

export function BucketForm({ mode, row, existing, onCancel, onDone, onError }: {
  mode: "create" | "edit"; row?: System; existing: System[];
  onCancel: () => void; onDone: () => Promise<void>; onError: (m: string | null) => void;
}) {
  const q = splitQuota(row?.quota_bytes ?? 0);
  const [name, setName]           = React.useState(row?.name ?? "");
  const [description, setDesc]    = React.useState(row?.description ?? "");
  const [tone, setTone]           = React.useState<Tone>(row?.tone ?? ("indigo" as Tone));
  const [bucket, setBucket]       = React.useState(row?.bucket ?? "");
  const [bucketTouched, setBT]    = React.useState(false);
  const [amount, setAmount]       = React.useState(q.amount);
  const [unit, setUnit]           = React.useState<Unit>(q.unit);
  const [unlimited, setUnlimited] = React.useState(q.unlimited);
  const [active, setActive]       = React.useState(row ? row.status === "live" : true);
  const [saving, setSaving]       = React.useState(false);

  const autoBucket = suggestBucket(name);
  const effectiveBucket = mode === "create" ? (bucketTouched ? bucket : autoBucket) : row!.bucket;
  const bucketTaken = mode === "create" && existing.some((s) => s.bucket === effectiveBucket);
  const bucketOk = mode === "edit" || (BUCKET_RE.test(effectiveBucket) && !bucketTaken);
  const quotaBytes = joinQuota(amount, unit, unlimited);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    onError(null);
    if (!name.trim()) { onError("Please enter a bucket name."); return; }
    if (!bucketOk) {
      onError(bucketTaken ? `Bucket id "${effectiveBucket}" already exists.` :
        "Bucket id may only contain a-z, 0-9 and - (2–63 chars), e.g. hr-contracts.");
      return;
    }
    if (quotaBytes == null) { onError("Enter a quota greater than 0, or tick “No limit”."); return; }
    setSaving(true);
    try {
      const body = {
        name: name.trim(),
        description: description.trim() || null,
        tone,
        quota_bytes: quotaBytes,
        ...(mode === "create" ? { bucket: effectiveBucket } : { status: active ? "live" : "paused" }),
      };
      const r = mode === "create"
        ? await fetch("/filehub/api/systems", { method: "POST", credentials: "include",
            headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) })
        : await mutate(`/filehub/api/systems/${encodeURIComponent(row!.id)}`, "PATCH", { credentials: "include",
            headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
      if (!r.ok) { onError((await r.json().catch(() => ({ error: r.statusText }))).error ?? r.statusText); return; }
      await onDone();
    } finally {
      setSaving(false);
    }
  };

  const label = (t: string, hint?: string) => (
    <div className="t-xs t-semibold" style={{ marginBottom: 4 }}>
      {t}{hint && <span className="t-muted" style={{ fontWeight: 400 }}> · {hint}</span>}
    </div>
  );

  return (
    <form onSubmit={submit} className="card" style={{ padding: 16, marginBottom: 12, background: "var(--bg-subtle, var(--bg-muted))", boxShadow: "none" }}>
      <div className="t-sm t-semibold" style={{ marginBottom: 12 }}>
        {mode === "create" ? "New bucket" : `Edit “${row!.name}”`}
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))", gap: 12 }}>
        <label>
          {label("Name", "shown to users")}
          <input className="field" value={name} onChange={(e) => setName(e.target.value)}
                 placeholder="e.g. HR – Contracts" style={{ width: "100%" }} autoFocus />
        </label>
        <label>
          {label("Bucket id", mode === "create" ? "a-z 0-9 - only, cannot be changed later" : "cannot be changed")}
          <input className="field t-mono" value={effectiveBucket} disabled={mode === "edit"}
                 onChange={(e) => { setBT(true); setBucket(e.target.value.toLowerCase()); }}
                 placeholder="e.g. hr-contracts" style={{ width: "100%", borderColor: bucketOk || !effectiveBucket ? undefined : "var(--danger)" }} />
          {mode === "create" && !bucketOk && effectiveBucket && (
            <div className="t-xs" style={{ color: "var(--danger)", marginTop: 4 }}>
              {bucketTaken ? "Already in use" : "Only a-z, 0-9 and -"}
            </div>
          )}
          {mode === "create" && !effectiveBucket && name && (
            <div className="t-xs t-muted" style={{ marginTop: 4 }}>Type a bucket id (the name has no Latin letters)</div>
          )}
        </label>
        <label style={{ gridColumn: "1 / -1" }}>
          {label("Description", "optional")}
          <input className="field" value={description} onChange={(e) => setDesc(e.target.value)}
                 placeholder="e.g. Signed employment contracts" style={{ width: "100%" }} />
        </label>
        <div>
          {label("Storage quota")}
          <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <input className="field t-tabular" value={amount} disabled={unlimited} inputMode="decimal"
                   onChange={(e) => setAmount(e.target.value)} placeholder="e.g. 50" style={{ width: 90 }} />
            <select className="field" value={unit} disabled={unlimited} onChange={(e) => setUnit(e.target.value as Unit)}>
              {UNITS.map((u) => <option key={u.v} value={u.v}>{u.v}</option>)}
            </select>
            <label className="t-sm" style={{ display: "inline-flex", alignItems: "center", gap: 4, marginLeft: 6 }}>
              <input type="checkbox" checked={unlimited} onChange={(e) => setUnlimited(e.target.checked)} /> No limit
            </label>
          </div>
        </div>
        <div>
          {label("Color")}
          <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
            {TONES.map((t) => (
              <button type="button" key={t.v} onClick={() => setTone(t.v)} title={t.label} aria-label={t.label}
                      className={"pill " + t.v + " sm"}
                      style={{ cursor: "pointer", outline: tone === t.v ? "2px solid var(--text)" : "none", outlineOffset: 1 }}>
                <span className="dot" />
              </button>
            ))}
          </div>
        </div>
        {mode === "edit" && (
          <label className="t-sm" style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <input type="checkbox" checked={active} onChange={(e) => setActive(e.target.checked)} /> Active (accepts new files)
          </label>
        )}
      </div>
      <div style={{ display: "flex", gap: 8, marginTop: 14 }}>
        <button type="submit" className="btn sm primary" disabled={saving}>
          {saving ? "Saving…" : mode === "create" ? "Create bucket" : "Save"}
        </button>
        <button type="button" className="btn sm ghost" onClick={onCancel} disabled={saving}>Cancel</button>
      </div>
    </form>
  );
}
