"use client";

import { useBranding } from "@/lib/branding";

import * as React from "react";
import { useParams } from "next/navigation";

import { bytes, when, BASE_PATH } from "@/lib/fh";

import "../../carbon.css";

type Meta = {
  name?: string; size_bytes?: number; file_type?: string; modified_at?: string; expires_at?: string | null; password_required?: boolean;
  downloads_left?: number | null; access?: "view" | "download"; login_required?: boolean; signed_in?: boolean; viewable?: boolean;
};

const MSG: Record<string, string> = {
  password_required: "Enter the password.", wrong_password: "Incorrect password.", login_required: "This link is only available to signed-in users.",
  view_only: "This link is view-only. Downloads aren't allowed.", not_viewable: "This file type can't be viewed in the browser.",
};

/// Public link page. The token is the credential; links can also need sign-in
/// and/or a password (sent in the body, never in the URL), and can be view-only.
export default function PublicLink() {
  const brand = useBranding();
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
      if (r.status === 429) throw new Error("Too many attempts to open links from your network. Wait 1 minute.");
      if (!r.ok) throw new Error("This link has expired, been turned off, or doesn't exist. Ask the sender for a new link.");
      setM(await r.json());
    }).catch((e) => setErr(e.message));
  }, [api]);
  const call = async (what: "download" | "view") => {
    setBusy(true); setMsg(null);
    try {
      const r = await fetch(`${api}/${what}`, { method: "POST", credentials: "same-origin", headers: { "content-type": "application/json" }, body: JSON.stringify({ password: pw || undefined }) });
      if (!r.ok) {
        let code = ""; try { code = (await r.json()).code ?? ""; } catch { /* not JSON */ }
        if (r.status === 429) throw new Error("Too many attempts. Wait 1 minute and try again.");
        throw new Error(MSG[code] ?? "This link can no longer be used.");
      }
      const blob = await r.blob();
      if (what === "view") {
        if (view) URL.revokeObjectURL(view.url);
        setView({ url: URL.createObjectURL(blob), mime: blob.type });
      } else {
        const name = /filename\*=UTF-8''([^;]+)/.exec(r.headers.get("content-disposition") ?? "")?.[1];
        const a = document.createElement("a"); a.href = URL.createObjectURL(blob); a.download = name ? decodeURIComponent(name) : (m?.name ?? "download"); a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 10000);
        setMsg({ ok: true, text: "Downloaded" });
        if (m?.downloads_left != null) setM({ ...m, downloads_left: Math.max(0, m.downloads_left - 1) });
      }
    } catch (e) { setMsg({ ok: false, text: (e as Error).message }); }
    setBusy(false);
  };
  const needLogin = m?.login_required && !m.signed_in;
  const canDownload = m?.access !== "view";
  return (
    <div className="cx-app" style={{ display: "block" }}>
      <div className="cx-nav-head">{brand.brand_name}</div>
      <div className="cx-page" style={{ maxWidth: view ? 1000 : 640, margin: "0 auto", paddingTop: 32 }}>
        {err ? <div className="cx-note cx-note-error" role="alert">{err}</div> : needLogin ? (
          <div className="cx-on-layer" style={{ background: "var(--cx-layer)", padding: 16, display: "flex", flexDirection: "column", gap: 12 }}>
            <h1 className="cx-title">This link is only available to signed-in users</h1>
            <div><a className="cx-btn cx-btn-primary" href={login}>Sign in</a></div>
          </div>
        ) : (
          <div className="cx-on-layer" style={{ background: "var(--cx-layer)", padding: 16, display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="cx-sub">Shared file · {m?.access === "view" ? "View only" : "View and download"}</div>
            <h1 className="cx-title" style={{ overflowWrap: "anywhere" }}>{m?.name ?? <span className="cx-skel" style={{ width: 240, height: 16 }} />}</h1>
            {m && <div className="cx-sub cx-num">{bytes(m.size_bytes)}{m.expires_at ? ` · Link valid until ${when(m.expires_at)}` : ""}{canDownload && m.downloads_left != null ? ` · ${m.downloads_left} downloads left` : ""}</div>}
            {m?.password_required && (
              <form onSubmit={(e) => { e.preventDefault(); call(canDownload ? "download" : "view"); }} className="cx-field" style={{ maxWidth: 320 }}>
                <label className="cx-label" htmlFor="pw">Password</label>
                <input id="pw" className="cx-input" type="password" autoComplete="off" value={pw} onChange={(e) => setPw(e.target.value)} />
              </form>
            )}
            {m && <div style={{ display: "flex", gap: 8 }}>
              {m.viewable && <button className={`cx-btn ${canDownload ? "cx-btn-secondary" : "cx-btn-primary"}`} disabled={busy} onClick={() => call("view")}>View</button>}
              {canDownload && <button className="cx-btn cx-btn-primary" disabled={busy} onClick={() => call("download")}>Download</button>}
            </div>}
            {msg && <div className={`cx-note ${msg.ok ? "cx-note-success" : "cx-note-error"}`} role="status">{msg.text}</div>}
            {view && (view.mime.startsWith("image/")
              ? <img src={view.url} alt={m?.name ?? ""} style={{ maxWidth: "100%" }} onContextMenu={(e) => { if (!canDownload) e.preventDefault(); }} />
              : view.mime.startsWith("video/") ? <video src={view.url} controls controlsList={canDownload ? undefined : "nodownload"} style={{ width: "100%" }} />
              : view.mime.startsWith("audio/") ? <audio src={view.url} controls controlsList={canDownload ? undefined : "nodownload"} />
              : <iframe title={m?.name ?? "File"} src={canDownload ? view.url : `${view.url}#toolbar=0`} style={{ width: "100%", height: "75vh", border: 0 }} />)}
          </div>
        )}
      </div>
    </div>
  );
}
