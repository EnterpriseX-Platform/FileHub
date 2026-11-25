"use client";

import * as React from "react";

import { api, when, BASE_PATH } from "@/lib/fh";
import { Btn, Check, Dropdown, Note, TextArea, TextField, useToast } from "./ui";

type Link = { token: string; title: string; message: string | null; require_login: boolean; expires_at: string; max_files: number | null; upload_count: number };

/// "Upload link" section of the Share dialog for a basket or folder.
export function UploadLinks({ systemId, folderId, placeName, bare }: { systemId: string; folderId: string | null; placeName: string; bare?: boolean }) {
  const toast = useToast();
  const [rows, setRows] = React.useState<Link[] | null>(null);
  const [open, setOpen] = React.useState(false);
  const [title, setTitle] = React.useState("");
  const [message, setMessage] = React.useState("");
  const [days, setDays] = React.useState("14");
  const [max, setMax] = React.useState("");
  const [login, setLogin] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);
  const q = `system_id=${encodeURIComponent(systemId)}${folderId ? `&folder_id=${encodeURIComponent(folderId)}` : ""}`;
  const load = React.useCallback(() => api<Link[]>(`/upload-links?${q}`).then(setRows).catch(() => setRows([])), [q]);
  React.useEffect(() => { load(); }, [load]);
  const url = (t: string) => `${window.location.origin}${BASE_PATH}/u/${t}`;
  const copy = async (t: string) => {
    try { await navigator.clipboard.writeText(url(t)); toast({ text: "Upload link copied." }); }
    catch { toast({ text: "Copy didn't work here. Select the link and copy it.", error: true }); }
  };
  const create = async () => {
    setErr(null);
    try {
      const r = await api<Link>("/upload-links", { method: "POST", json: {
        system_id: systemId, folder_id: folderId, title: title || `Files for ${placeName}`, message: message || undefined,
        expires_in_days: Number(days), max_files: max ? Number(max) : undefined, require_login: login,
      } });
      setOpen(false); setTitle(""); setMessage(""); setMax(""); await load(); copy(r.token);
    } catch (e) { setErr((e as Error).message); }
  };
  return (
    <div style={bare ? { display: "flex", flexDirection: "column", gap: 8 } : { borderTop: "1px solid var(--cx-border-subtle)", paddingTop: 12, display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <div style={{ flex: 1 }}>{!bare && <div>Upload link</div>}{rows && rows.length === 0 && bare && <span className="cx-sub">No active links</span>}</div>
        {!open && <Btn kind="tertiary" onClick={() => setOpen(true)}>Create upload link</Btn>}
      </div>
      {rows && rows.length > 0 && (
        <ul className="cx-list" style={{ background: "var(--cx-layer-2)", padding: "0 12px" }}>
          {rows.map((l) => (
            <li key={l.token}>
              <span className="cx-grow">{l.title}
                <div className="cx-helper cx-num">{l.upload_count} file{l.upload_count === 1 ? "" : "s"} received{l.max_files ? ` of ${l.max_files}` : ""} · until {when(l.expires_at, false)}{l.require_login ? " · sign-in required" : ""}</div>
              </span>
              <Btn kind="ghost" onClick={() => copy(l.token)}>Copy link</Btn>
              <Btn kind="danger-ghost" onClick={async () => { await api(`/upload-links/${l.token}`, { method: "DELETE" }); toast({ text: "Upload link turned off." }); load(); }}>Turn off</Btn>
            </li>
          ))}
        </ul>
      )}
      {open && (
        <div className="cx-on-layer" style={{ background: "var(--cx-layer-2)", padding: 12, display: "flex", flexDirection: "column", gap: 10 }}>
          <TextField label="Title people see" placeholder={`Files for ${placeName}`} value={title} onChange={(e) => setTitle(e.target.value)} />
          <TextArea label="Message (optional)" value={message} onChange={(e) => setMessage(e.target.value)} />
          <div className="cx-form-grid">
            <Dropdown label="Link works for" value={days} options={[{ value: "1", label: "1 day" }, { value: "7", label: "7 days" }, { value: "14", label: "14 days" }, { value: "30", label: "30 days" }, { value: "90", label: "90 days" }]} onChange={setDays} />
            <TextField label="Most files it accepts (optional)" inputMode="numeric" value={max} onChange={(e) => setMax(e.target.value.replace(/\D/g, ""))} />
          </div>
          <Check label="Only people signed in to the organization" checked={login} onChange={setLogin} />
          {err && <Note kind="error">{err}</Note>}
          <div style={{ display: "flex", gap: 8 }}><Btn kind="primary" onClick={create}>Create and copy link</Btn><Btn kind="ghost" onClick={() => setOpen(false)}>Cancel</Btn></div>
        </div>
      )}
    </div>
  );
}
