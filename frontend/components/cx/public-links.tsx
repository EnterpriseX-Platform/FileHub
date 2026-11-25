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
  const copy = async (t: string) => { try { await navigator.clipboard.writeText(url(t)); toast({ text: "คัดลอกลิงก์แล้ว" }); } catch { toast({ text: "คัดลอกไม่สำเร็จ กรุณาเลือกลิงก์แล้วคัดลอกเอง", error: true }); } };
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <div style={{ flex: 1 }}>ลิงก์แชร์</div>
        {!open && <Btn kind="ghost" onClick={() => { setOpen(true); setMade(null); }}>สร้างลิงก์แชร์</Btn>}
      </div>
      {rows && rows.length > 0 && (
        <ul className="cx-list" style={{ background: "var(--cx-layer-2)", padding: "0 12px" }}>
          {rows.map((l) => (
            <li key={l.id}>
              <span className="cx-grow cx-num">{l.access === "view" ? "ดูอย่างเดียว" : "ดูและดาวน์โหลด"} · {l.require_login ? "เฉพาะผู้ใช้ในระบบ" : "ทุกคนที่มีลิงก์"} · ถึง {when(l.expires_at, false)}{l.has_password ? " · มีรหัสผ่าน" : ""} · ดาวน์โหลด {l.download_count}{l.max_downloads ? `/${l.max_downloads}` : ""} ครั้ง
                <div className="cx-helper">{l.created_by}{l.last_access_at ? ` · ใช้ล่าสุด ${when(l.last_access_at)}` : ""}</div></span>
              <Btn kind="ghost" onClick={() => copy(l.token)}>คัดลอก</Btn>
              <Btn kind="danger-ghost" onClick={async () => { try { await api(`/share-links/${l.id}`, { method: "DELETE" }); toast({ text: "ปิดลิงก์แล้ว" }); load(); } catch (e) { toast({ text: (e as Error).message, error: true }); } }}>ปิดลิงก์</Btn>
            </li>
          ))}
        </ul>
      )}
      {open && (
        <div className="cx-on-layer" style={{ background: "var(--cx-layer-2)", padding: 12, display: "flex", flexDirection: "column", gap: 10 }}>
          <div className="cx-form-grid">
            <Dropdown label="สิทธิ์ของผู้รับ" value={access} options={[{ value: "view", label: "ดูได้อย่างเดียว" }, { value: "download", label: "ดูและดาวน์โหลด" }]} onChange={(v) => setAccess(v as "view" | "download")} />
            <Dropdown label="ผู้ที่ใช้ลิงก์ได้" value={who} options={[{ value: "login", label: "เฉพาะผู้ใช้ที่เข้าสู่ระบบ" }, { value: "anyone", label: "ทุกคนที่มีลิงก์" }]} onChange={(v) => setWho(v as "login" | "anyone")} />
            <Dropdown label="ใช้ลิงก์ได้" value={days} options={[{ value: "1", label: "1 วัน" }, { value: "3", label: "3 วัน" }, { value: "7", label: "7 วัน" }, { value: "30", label: "30 วัน" }, { value: "90", label: "90 วัน" }]} onChange={setDays} />
            <TextField label="ดาวน์โหลดได้สูงสุด (ครั้ง)" inputMode="numeric" value={max} onChange={(e) => setMax(e.target.value.replace(/\D/g, ""))} />
            <TextField label="รหัสผ่าน (อย่างน้อย 6 ตัว)" type="password" autoComplete="new-password" value={pw} onChange={(e) => setPw(e.target.value)} />
          </div>
          {err && <Note kind="error">{err}</Note>}
          <div style={{ display: "flex", gap: 8 }}>
            <Btn kind="primary" onClick={async () => {
              setErr(null);
              try {
                const r = await api<{ token: string }>(`/files/${fileId}/share`, { method: "POST", json: { expires_in_days: Number(days), password: pw || undefined, max_downloads: max ? Number(max) : undefined, access, require_login: who === "login" } });
                setOpen(false); setPw(""); setMax(""); setMade(url(r.token)); load(); copy(r.token);
              } catch (e) { setErr((e as Error).message); }
            }}>สร้างและคัดลอกลิงก์</Btn>
            <Btn kind="ghost" onClick={() => setOpen(false)}>ยกเลิก</Btn>
          </div>
        </div>
      )}
      {made && <TextField label="ลิงก์ที่สร้าง" readOnly value={made} onFocus={(e) => e.target.select()} />}
    </div>
  );
}
