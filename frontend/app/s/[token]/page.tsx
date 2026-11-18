"use client";

import * as React from "react";
import { useParams } from "next/navigation";

import { bytes, when, BASE_PATH } from "@/lib/fh";

import "../../carbon.css";

type Meta = {
  name?: string; size_bytes?: number; file_type?: string; modified_at?: string; expires_at?: string | null; password_required?: boolean;
  downloads_left?: number | null; access?: "view" | "download"; login_required?: boolean; signed_in?: boolean; viewable?: boolean;
};

const MSG: Record<string, string> = {
  password_required: "กรุณาใส่รหัสผ่าน", wrong_password: "รหัสผ่านไม่ถูกต้อง", login_required: "ลิงก์นี้ใช้ได้เฉพาะผู้ที่เข้าสู่ระบบแล้ว",
  view_only: "ลิงก์นี้ให้เปิดดูได้อย่างเดียว ดาวน์โหลดไม่ได้", not_viewable: "ไฟล์ประเภทนี้เปิดดูในเบราว์เซอร์ไม่ได้",
};

/// Public link page. The token is the credential; links can also need sign-in
/// and/or a password (sent in the body, never in the URL), and can be view-only.
export default function PublicLink() {
  const { token } = useParams<{ token: string }>();
  const [m, setM] = React.useState<Meta | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [pw, setPw] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [msg, setMsg] = React.useState<{ ok: boolean; text: string } | null>(null);
  const [view, setView] = React.useState<{ url: string; mime: string } | null>(null);
  const api = `${BASE_PATH}/api/share/${encodeURIComponent(token)}`;
  const login = `${BASE_PATH}/login?next=${encodeURIComponent(`/s/${token}`)}`;
  React.useEffect(() => {
    fetch(api, { credentials: "same-origin" }).then(async (r) => {
      if (r.status === 429) throw new Error("มีการลองเปิดลิงก์จากเครือข่ายของคุณมากเกินไป กรุณารอ 1 นาที");
      if (!r.ok) throw new Error("ลิงก์นี้หมดอายุ ถูกปิด หรือไม่มีอยู่ กรุณาขอลิงก์ใหม่จากผู้ส่ง");
      setM(await r.json());
    }).catch((e) => setErr(e.message));
  }, [api]);
  const call = async (what: "download" | "view") => {
    setBusy(true); setMsg(null);
    try {
      const r = await fetch(`${api}/${what}`, { method: "POST", credentials: "same-origin", headers: { "content-type": "application/json" }, body: JSON.stringify({ password: pw || undefined }) });
      if (!r.ok) {
        let code = ""; try { code = (await r.json()).code ?? ""; } catch { /* not JSON */ }
        if (r.status === 429) throw new Error("ลองหลายครั้งเกินไป กรุณารอ 1 นาทีแล้วลองใหม่");
        throw new Error(MSG[code] ?? "ใช้ลิงก์นี้ไม่ได้แล้ว");
      }
      const blob = await r.blob();
      if (what === "view") {
        if (view) URL.revokeObjectURL(view.url);
        setView({ url: URL.createObjectURL(blob), mime: blob.type });
      } else {
        const name = /filename\*=UTF-8''([^;]+)/.exec(r.headers.get("content-disposition") ?? "")?.[1];
        const a = document.createElement("a"); a.href = URL.createObjectURL(blob); a.download = name ? decodeURIComponent(name) : (m?.name ?? "download"); a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 10000);
        setMsg({ ok: true, text: "ดาวน์โหลดแล้ว" });
        if (m?.downloads_left != null) setM({ ...m, downloads_left: Math.max(0, m.downloads_left - 1) });
      }
    } catch (e) { setMsg({ ok: false, text: (e as Error).message }); }
    setBusy(false);
  };
  const needLogin = m?.login_required && !m.signed_in;
  const canDownload = m?.access !== "view";
  return (
    <div className="cx-app" style={{ display: "block" }}>
      <div className="cx-nav-head">FileHub</div>
      <div className="cx-page" style={{ maxWidth: view ? 1000 : 640, margin: "0 auto", paddingTop: 32 }}>
        {err ? <div className="cx-note cx-note-error" role="alert">{err}</div> : needLogin ? (
          <div className="cx-on-layer" style={{ background: "var(--cx-layer)", padding: 16, display: "flex", flexDirection: "column", gap: 12 }}>
            <h1 className="cx-title">ลิงก์นี้ใช้ได้เฉพาะผู้ที่เข้าสู่ระบบ</h1>
            <div><a className="cx-btn cx-btn-primary" href={login}>เข้าสู่ระบบ</a></div>
          </div>
        ) : (
          <div className="cx-on-layer" style={{ background: "var(--cx-layer)", padding: 16, display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="cx-sub">ไฟล์ที่แชร์ · {m?.access === "view" ? "ดูได้อย่างเดียว" : "ดูและดาวน์โหลด"}</div>
            <h1 className="cx-title" style={{ overflowWrap: "anywhere" }}>{m?.name ?? <span className="cx-skel" style={{ width: 240, height: 16 }} />}</h1>
            {m && <div className="cx-sub cx-num">{bytes(m.size_bytes)}{m.expires_at ? ` · ใช้ลิงก์ได้ถึง ${when(m.expires_at)}` : ""}{canDownload && m.downloads_left != null ? ` · ดาวน์โหลดได้อีก ${m.downloads_left} ครั้ง` : ""}</div>}
            {m?.password_required && (
              <form onSubmit={(e) => { e.preventDefault(); call(canDownload ? "download" : "view"); }} className="cx-field" style={{ maxWidth: 320 }}>
                <label className="cx-label" htmlFor="pw">รหัสผ่าน</label>
                <input id="pw" className="cx-input" type="password" autoComplete="off" value={pw} onChange={(e) => setPw(e.target.value)} />
              </form>
            )}
            {m && <div style={{ display: "flex", gap: 8 }}>
              {m.viewable && <button className={`cx-btn ${canDownload ? "cx-btn-secondary" : "cx-btn-primary"}`} disabled={busy} onClick={() => call("view")}>เปิดดู</button>}
              {canDownload && <button className="cx-btn cx-btn-primary" disabled={busy} onClick={() => call("download")}>ดาวน์โหลด</button>}
            </div>}
            {msg && <div className={`cx-note ${msg.ok ? "cx-note-success" : "cx-note-error"}`} role="status">{msg.text}</div>}
            {view && (view.mime.startsWith("image/")
              ? <img src={view.url} alt={m?.name ?? ""} style={{ maxWidth: "100%" }} onContextMenu={(e) => { if (!canDownload) e.preventDefault(); }} />
              : view.mime.startsWith("video/") ? <video src={view.url} controls controlsList={canDownload ? undefined : "nodownload"} style={{ width: "100%" }} />
              : view.mime.startsWith("audio/") ? <audio src={view.url} controls controlsList={canDownload ? undefined : "nodownload"} />
              : <iframe title={m?.name ?? "ไฟล์"} src={canDownload ? view.url : `${view.url}#toolbar=0`} style={{ width: "100%", height: "75vh", border: 0 }} />)}
          </div>
        )}
      </div>
    </div>
  );
}
