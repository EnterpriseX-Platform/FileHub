"use client";

import * as React from "react";

import { uploadChunked } from "@/lib/chunked";
import Link from "next/link";

import { api, bytes, meta, when, BASE_PATH, type Effective, type FileRow, type Settings } from "@/lib/fh";
import { cleanFields, FieldInputs } from "./modals";
import { Btn, I, Note, StatusTag, Tabs, useToast } from "./ui";

type Version = { id: string; version: number; size_bytes: number; uploaded_by: string | null; note: string | null; created_at: string };
type Act = { id: string; actor: string; action: string; target: string | null; created_at: string };
type Appears = { id: string; name: string; match_mode: string; rules: unknown[] };
type TabId = "details" | "versions" | "access" | "activity" | "appears";

/// Side panel for one file: Details · Versions · Access · Activity · Appears in.
export function FilePanel({ fileId, settingsFor, onClose, onChanged, onShare, onStatus, basketName, folderPath, refreshKey = 0 }: {
  fileId: string; refreshKey?: number; settingsFor: (basket: string) => Promise<Settings | null>; onClose: () => void; onChanged: () => void;
  onShare: (f: FileRow) => void; onStatus: (f: FileRow) => void; basketName?: string; folderPath?: string;
}) {
  const toast = useToast();
  const [f, setF] = React.useState<FileRow | null>(null);
  const [eff, setEff] = React.useState<Effective | null>(null);
  const [settings, setSettings] = React.useState<Settings | null>(null);
  const [tab, setTab] = React.useState<TabId>("details");
  const [err, setErr] = React.useState<string | null>(null);
  const [verPct, setVerPct] = React.useState<number | null>(null);
  const [editing, setEditing] = React.useState(false);
  const [vals, setVals] = React.useState<Record<string, string>>({});
  const [cat, setCat] = React.useState("");
  const [versions, setVersions] = React.useState<Version[] | null>(null);
  const [acts, setActs] = React.useState<Act[] | null>(null);
  const [appears, setAppears] = React.useState<Appears[] | null>(null);
  const [acl, setAcl] = React.useState<{ entries: { id: string; principal_label: string; level_name: string }[]; inherited: { id: string; principal_label: string; level_name: string; inherited_from: string | null }[] } | null>(null);
  const [tick, setTick] = React.useState(0);
  const verInput = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    let off = false;
    setErr(null); setEditing(false); setVersions(null); setActs(null); setAppears(null); setAcl(null);
    (async () => {
      try {
        const file = await api<FileRow>(`/files/${fileId}`);
        const [e, s] = await Promise.all([api<Effective>(`/acl/file/${fileId}/effective`), settingsFor(file.system_id)]);
        if (off) return;
        setF(file); setEff(e); setSettings(s);
        const m = meta(file.metadata);
        setVals(Object.fromEntries(Object.entries(m).map(([k, v]) => [k, v == null ? "" : String(v)])));
        setCat(file.category_code ?? "");
      } catch (e) { if (!off) setErr((e as Error).message); }
    })();
    return () => { off = true; };
  }, [fileId, tick, settingsFor, refreshKey]);

  React.useEffect(() => {
    if (tab === "versions" && !versions) api<Version[]>(`/files/${fileId}/versions`).then(setVersions).catch(() => setVersions([]));
    if (tab === "activity" && !acts) api<Act[]>(`/files/${fileId}/activity`).then(setActs).catch(() => setActs([]));
    if (tab === "appears" && !appears) api<Appears[]>(`/files/${fileId}/appears-in`).then(setAppears).catch(() => setAppears([]));
    if (tab === "access" && !acl) api<typeof acl>(`/acl/file/${fileId}`).then(setAcl).catch(() => setAcl({ entries: [], inherited: [] }));
  }, [tab, fileId, versions, acts, appears, acl]);

  const can = (a: string) => eff?.actions.includes(a);
  const status = settings?.statuses.find((s) => s.code === f?.status);
  const locked = !!status?.locks_content;
  const catName = settings?.categories.find((c) => c.code === f?.category_code)?.name;
  const m = f ? meta(f.metadata) : {};

  const saveFields = async () => {
    if (!f || !settings) return;
    try {
      const all = cleanFields(settings.fields, vals);
      const removed = Object.keys(meta(f.metadata)).filter((k) => !(k in all) && settings.fields.some((d) => d.key === k));
      const fields: Record<string, unknown> = { ...all };
      for (const k of removed) fields[k] = null;
      await api(`/files/${f.id}`, { method: "PATCH", json: { fields, category_code: cat || null } });
      toast({ text: "Details saved." }); setEditing(false); setTick((t) => t + 1); onChanged();
    } catch (e) { setErr((e as Error).message); }
  };
  const newVersion = async (file: File) => {
    if (!f) return;
    setVerPct(0);
    try {
      await uploadChunked({ endpoint: "/api/uploads", file, meta: { purpose: "version", file_id: f.id }, onProgress: (n, t) => setVerPct(t ? Math.round((n / t) * 100) : 100) });
      setVerPct(null);
      toast({ text: `Version uploaded.` }); setVersions(null); setTick((t) => t + 1); onChanged();
    } catch (e) { setVerPct(null); setErr((e as Error).message); }
  };

  return (
    <aside className="cx-panel" aria-label="File details">
      <div className="cx-panel-head">
        <h2>{f?.name ?? (err ? "File" : <span className="cx-skel" style={{ width: 180, height: 14 }} />)}</h2>
        <button className="cx-close" aria-label="Close details" onClick={onClose}><I.close /></button>
      </div>
      {f && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: 0, borderBottom: "1px solid var(--cx-border-subtle)" }}>
          <a className="cx-btn cx-btn-ghost" href={`${BASE_PATH}/api/files/${f.id}/download`}>Download</a>
          <Link className="cx-btn cx-btn-ghost" href={`/view/${f.id}`}>Open</Link>
          {can("change_status") && <Btn kind="ghost" onClick={() => onStatus(f)}>Change status</Btn>}
          {(can("share") || can("forward")) && <Btn kind="ghost" onClick={() => onShare(f)}>Share</Btn>}
        </div>
      )}
      <Tabs<TabId> value={tab} onChange={setTab} tabs={[
        { id: "details", label: "Details" }, { id: "versions", label: "Versions" }, { id: "access", label: "Access" },
        { id: "activity", label: "Activity" }, { id: "appears", label: "Appears in" },
      ]} />
      <div className="cx-panel-body">
        {err && <Note kind="error">{err}</Note>}
        {f && tab === "details" && (
          editing && settings ? (
            <div className="cx-on-layer" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <FieldInputs fields={settings.fields} values={vals} onChange={(k, v) => setVals((x) => ({ ...x, [k]: v }))}
                categories={settings.categories} category={cat} onCategory={setCat} />
              <div style={{ display: "flex", gap: 8 }}>
                <Btn kind="primary" onClick={saveFields}>Save</Btn>
                <Btn kind="ghost" onClick={() => setEditing(false)}>Cancel</Btn>
              </div>
            </div>
          ) : (
            <>
              <dl className="cx-kv">
                <dt>Status</dt><dd><StatusTag code={f.status} statuses={settings?.statuses} />{locked && <span className="cx-helper"> · content locked</span>}</dd>
                <dt>Category</dt><dd>{catName ?? "—"}</dd>
                <dt>Stored in</dt><dd>{basketName ?? f.system_id}{folderPath ? ` › ${folderPath}` : ""}</dd>
                <dt>Uploaded by</dt><dd>{f.owner}</dd>
                <dt>Created</dt><dd className="cx-num">{when(f.created_at)}</dd>
                <dt>Modified</dt><dd className="cx-num">{when(f.modified_at)}</dd>
                <dt>Size</dt><dd className="cx-num">{bytes(f.size_bytes)} · version {f.version}</dd>
                <dt>Encrypted</dt><dd>{f.encrypted ? "Yes" : "No"}</dd>
                <dt>Your access</dt><dd style={{ textTransform: "capitalize" }}>{eff?.level}</dd>
              </dl>
              {settings && settings.fields.length > 0 && <>
                <div className="cx-section-label">Fields</div>
                <dl className="cx-kv">
                  {settings.fields.map((d) => (
                    <React.Fragment key={d.key}><dt>{d.label}</dt><dd>{m[d.key] == null || m[d.key] === "" ? <span className="cx-helper">—</span> : String(m[d.key])}</dd></React.Fragment>
                  ))}
                </dl>
              </>}
              {can("edit") && (!locked || can("manage_access")) && settings && (settings.fields.length > 0 || settings.categories.length > 0) && (
                <div><Btn kind="tertiary" onClick={() => setEditing(true)}>Edit details</Btn></div>
              )}
            </>
          )
        )}
        {f && tab === "versions" && (
          <>
            {can("upload_version") && (
              locked ? <Note>Status “{status?.label}” locks this file. Change the status to upload a new version.</Note>
                : <div><Btn kind="tertiary" disabled={verPct != null} onClick={() => verInput.current?.click()}>{verPct != null ? `Uploading ${verPct}%` : "Upload new version"}</Btn>
                  {verPct != null && <div className="cx-meter" style={{ marginTop: 4 }}><i style={{ width: `${verPct}%` }} /></div>}
                  <input ref={verInput} type="file" hidden onChange={(e) => { const x = e.target.files?.[0]; if (x) newVersion(x); e.target.value = ""; }} /></div>
            )}
            <ul className="cx-list">
              {(() => { const c = versions?.find((v) => v.version === f.version); return (
              <li><span className="cx-grow"><b style={{ fontWeight: 600 }}>เวอร์ชัน {f.version}</b> · ปัจจุบัน{c?.note ? ` · ${c.note}` : ""}<div className="cx-helper cx-num">{when(f.modified_at)} · {bytes(f.size_bytes)}{c?.uploaded_by ? ` · ${c.uploaded_by}` : ""}</div></span>
                <a className="cx-btn cx-btn-ghost" href={`${BASE_PATH}/api/files/${f.id}/download`}>ดาวน์โหลด</a></li>); })()}
              {versions == null && <li><span className="cx-skel" style={{ width: "70%" }} /></li>}
              {versions?.filter((v) => v.version !== f.version).map((v) => (
                <li key={v.id}><span className="cx-grow">เวอร์ชัน {v.version}{v.note ? ` · ${v.note}` : ""}<div className="cx-helper cx-num">{when(v.created_at)} · {bytes(v.size_bytes)}{v.uploaded_by ? ` · ${v.uploaded_by}` : ""}</div></span>
                  <a className="cx-btn cx-btn-ghost" href={`${BASE_PATH}/api/files/${f.id}/download?version=${v.version}`}>ดาวน์โหลด</a></li>
              ))}
            </ul>
          </>
        )}
        {f && tab === "access" && (
          <>
            <ul className="cx-list">
              {acl == null && <li><span className="cx-skel" style={{ width: "60%" }} /></li>}
              {acl?.entries.map((e) => <li key={e.id}><span className="cx-grow">{e.principal_label}</span><span className="cx-sub" style={{ textTransform: "capitalize" }}>{e.level_name}</span></li>)}
              {acl?.inherited.map((e) => <li key={e.id}><span className="cx-grow">{e.principal_label}<div className="cx-helper">from {e.inherited_from}</div></span><span className="cx-sub" style={{ textTransform: "capitalize" }}>{e.level_name}</span></li>)}
              {acl && acl.entries.length === 0 && acl.inherited.length === 0 && <li className="cx-sub">Basket roles apply. No one has been given access here.</li>}
            </ul>
            {(can("share") || can("forward")) && <div><Btn kind="tertiary" onClick={() => onShare(f)}>Manage access</Btn></div>}
          </>
        )}
        {f && tab === "activity" && (
          <ul className="cx-list">
            {acts == null && <li><span className="cx-skel" style={{ width: "70%" }} /></li>}
            {acts?.length === 0 && <li className="cx-sub">No activity yet.</li>}
            {acts?.map((a) => <li key={a.id}><span className="cx-grow">{a.actor} {a.action}<div className="cx-helper cx-num">{when(a.created_at)}</div></span></li>)}
          </ul>
        )}
        {f && tab === "appears" && (
          <>
            <dl className="cx-kv"><dt>Stored in</dt><dd>{basketName ?? f.system_id}{folderPath ? ` › ${folderPath}` : ""}</dd></dl>
            <div className="cx-section-label">Virtual baskets</div>
            <ul className="cx-list">
              {appears == null && <li><span className="cx-skel" style={{ width: "60%" }} /></li>}
              {appears?.length === 0 && <li className="cx-sub">Not in any virtual basket you can open.</li>}
              {appears?.map((v) => <li key={v.id}><I.vbasket /><span className="cx-grow"><Link href={`/v/${v.id}`} style={{ color: "var(--cx-interactive)" }}>{v.name}</Link><div className="cx-helper">{describeRules(v.match_mode, v.rules)}</div></span></li>)}
            </ul>
          </>
        )}
      </div>
    </aside>
  );
}

const OPS: Record<string, string> = {
  eq: "is", ne: "is not", in: "is any of", nin: "is none of", contains: "contains", starts: "starts with",
  gt: ">", gte: "≥", lt: "<", lte: "≤", before: "before", after: "after", last_days: "in last", empty: "is empty", not_empty: "is not empty",
};
export function describeRules(mode: string, rules: unknown[]): string {
  const one = (r: unknown): string => {
    const x = r as { field?: string; op?: string; value?: unknown; match?: string; rules?: unknown[] };
    if (x.rules) return `(${describeRules(x.match ?? "all", x.rules)})`;
    const field = (x.field ?? "").replace(/^sys\./, "").replace(/_/g, " ");
    const v = Array.isArray(x.value) ? x.value.join(", ") : x.value == null ? "" : String(x.value);
    return `${field.charAt(0).toUpperCase()}${field.slice(1)} ${OPS[x.op ?? ""] ?? x.op}${v ? ` ${v}` : ""}${x.op === "last_days" ? " days" : ""}`;
  };
  return (rules ?? []).map(one).join(mode === "any" ? " · or · " : " · ");
}
