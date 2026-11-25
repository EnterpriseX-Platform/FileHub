"use client";

import * as React from "react";

import { api, when, BASE_PATH } from "@/lib/fh";
import { Btn, Dropdown, Note, TextField, useToast } from "./ui";

type L = { id: string; token: string; expires_at: string | null; created_by: string; max_downloads: number | null; download_count: number; has_password: boolean; last_access_at: string | null; access: "view" | "download"; require_login: boolean };

/// Public links of one file: create (expiry, password, most downloads), list, turn off.
export function PublicLinks({ fileId }: { fileId: string }) {
  const toast = useToast();
  const [rows, setRows] = React.useState<L[] | null>(null);
  const [open, setOpen] = React.useState(false);
  const [days, setDays] = React.useState("7");
  const [pw, setPw] = React.useState("");
  const [max, setMax] = React.useState("");
  const [access, setAccess] = React.useState<"view" | "download">("view");
  const [who, setWho] = React.useState<"login" | "anyone">("login");
  const [err, setErr] = React.useState<string | null>(null);
  const [made, setMade] = React.useState<string | null>(null);
  const load = React.useCallback(() => api<L[]>(`/files/${fileId}/links`).then(setRows).catch(() => setRows([])), [fileId]);
  React.useEffect(() => { load(); }, [load]);
  const url = (t: string) => `${window.location.origin}${BASE_PATH}/s/${t}`;
  const copy = async (t: string) => { try { await navigator.clipboard.writeText(url(t)); toast({ text: "Link copied" }); } catch { toast({ text: "Couldn't copy. Select the link and copy it manually.", error: true }); } };
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <div style={{ flex: 1 }}>Share links</div>
        {!open && <Btn kind="ghost" onClick={() => { setOpen(true); setMade(null); }}>Create share link</Btn>}
      </div>
      {rows && rows.length > 0 && (
        <ul className="cx-list" style={{ background: "var(--cx-layer-2)", padding: "0 12px" }}>
          {rows.map((l) => (
            <li key={l.id}>
              <span className="cx-grow cx-num">{l.access === "view" ? "View only" : "View and download"} · {l.require_login ? "Signed-in users only" : "Anyone with the link"} · Until {when(l.expires_at, false)}{l.has_password ? " · Password protected" : ""} · Downloads {l.download_count}{l.max_downloads ? `/${l.max_downloads}` : ""}
                <div className="cx-helper">{l.created_by}{l.last_access_at ? ` · Last used ${when(l.last_access_at)}` : ""}</div></span>
              <Btn kind="ghost" onClick={() => copy(l.token)}>Copy</Btn>
              <Btn kind="danger-ghost" onClick={async () => { try { await api(`/share-links/${l.id}`, { method: "DELETE" }); toast({ text: "Link turned off" }); load(); } catch (e) { toast({ text: (e as Error).message, error: true }); } }}>Turn off</Btn>
            </li>
          ))}
        </ul>
      )}
      {open && (
        <div className="cx-on-layer" style={{ background: "var(--cx-layer-2)", padding: 12, display: "flex", flexDirection: "column", gap: 10 }}>
          <div className="cx-form-grid">
            <Dropdown label="Recipient access" value={access} options={[{ value: "view", label: "View only" }, { value: "download", label: "View and download" }]} onChange={(v) => setAccess(v as "view" | "download")} />
            <Dropdown label="Who can use the link" value={who} options={[{ value: "login", label: "Signed-in users only" }, { value: "anyone", label: "Anyone with the link" }]} onChange={(v) => setWho(v as "login" | "anyone")} />
            <Dropdown label="Link expires after" value={days} options={[{ value: "1", label: "1 day" }, { value: "3", label: "3 days" }, { value: "7", label: "7 days" }, { value: "30", label: "30 days" }, { value: "90", label: "90 days" }]} onChange={setDays} />
            <TextField label="Max downloads" inputMode="numeric" value={max} onChange={(e) => setMax(e.target.value.replace(/\D/g, ""))} />
            <TextField label="Password (at least 6 characters)" type="password" autoComplete="new-password" value={pw} onChange={(e) => setPw(e.target.value)} />
          </div>
          {err && <Note kind="error">{err}</Note>}
          <div style={{ display: "flex", gap: 8 }}>
            <Btn kind="primary" onClick={async () => {
              setErr(null);
              try {
                const r = await api<{ token: string }>(`/files/${fileId}/share`, { method: "POST", json: { expires_in_days: Number(days), password: pw || undefined, max_downloads: max ? Number(max) : undefined, access, require_login: who === "login" } });
                setOpen(false); setPw(""); setMax(""); setMade(url(r.token)); load(); copy(r.token);
              } catch (e) { setErr((e as Error).message); }
            }}>Create and copy link</Btn>
            <Btn kind="ghost" onClick={() => setOpen(false)}>Cancel</Btn>
          </div>
        </div>
      )}
      {made && <TextField label="Created link" readOnly value={made} onFocus={(e) => e.target.select()} />}
    </div>
  );
}
