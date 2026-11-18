"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";

import { api, bytes, ext, BASE_PATH, type Basket, type FileRow, type Settings } from "@/lib/fh";
import { loadSettings } from "./basket-view";
import { Btn, I, Note, StatusTag, Tabs, When } from "./ui";

type Kind = "pdf" | "office" | "image" | "audio" | "video" | "html" | "text" | "none";

/// What the viewer does with each extension (TOR 4.15.6 types first).
export function kindOf(name: string): Kind {
  const e = (/\.([^./]+)$/.exec(name)?.[1] ?? "").toLowerCase();
  if (e === "pdf") return "pdf";
  if (["doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf"].includes(e)) return "office";
  if (["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"].includes(e)) return "image";
  if (["wav", "mp3", "m4a", "ogg", "aac", "flac"].includes(e)) return "audio";
  if (["mp4", "webm", "mov"].includes(e)) return "video";
  if (["html", "htm"].includes(e)) return "html";
  if (["xml", "txt", "csv", "json", "md", "log"].includes(e)) return "text";
  return "none";
}

const TEXT_LIMIT = 2 * 1024 * 1024;

/// FH-05 Preview: opens every TOR file type inside FileHub.
/// PDF and Office (converted to PDF on the server) use the browser's PDF viewer,
/// images/audio/video play natively, HTML is shown as source or rendered in a
/// sandbox with scripts off, XML/text as text.
export function FileViewer({ id }: { id: string }) {
  const router = useRouter();
  const [f, setF] = React.useState<FileRow | null>(null);
  const [basket, setBasket] = React.useState<Basket | null>(null);
  const [folderName, setFolderName] = React.useState<string | null>(null);
  const [settings, setSettings] = React.useState<Settings | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [text, setText] = React.useState<string | null>(null);
  const [mode, setMode] = React.useState<"rendered" | "source">("rendered");
  const [office, setOffice] = React.useState<"loading" | "ok" | "failed">("loading");

  React.useEffect(() => {
    let off = false;
    api<FileRow>(`/files/${id}`).then(async (file) => {
      if (off) return;
      setF(file);
      const [all, s] = await Promise.all([api<Basket[]>("/systems").catch(() => []), loadSettings(file.system_id)]);
      if (off) return;
      setBasket(all.find((b) => b.id === file.system_id) ?? null);
      setSettings(s);
      if (file.folder_id) api<{ name?: string; folder?: { name: string } }>(`/folders/${file.folder_id}`)
        .then((fd) => { if (!off) setFolderName(fd.name ?? fd.folder?.name ?? null); }).catch(() => {});
    }).catch((e) => setErr(e.message));
    return () => { off = true; };
  }, [id]);

  const kind = f ? kindOf(f.name) : "none";
  const dl = `${BASE_PATH}/api/files/${id}/download`;
  const preview = `${BASE_PATH}/api/files/${id}/preview`;

  // Text-like files: fetch and show as text (never injected as HTML).
  React.useEffect(() => {
    if (!f || (kind !== "text" && kind !== "html")) return;
    if (f.size_bytes > TEXT_LIMIT) { setText(null); return; }
    fetch(dl).then(async (r) => {
      if (!r.ok) throw new Error(`Couldn't open the file (${r.status}).`);
      const buf = await r.arrayBuffer();
      let t = new TextDecoder("utf-8", { fatal: false }).decode(buf);
      if (/\.xml$/i.test(f.name) || /\.json$/i.test(f.name)) t = pretty(f.name, t);
      setText(t);
    }).catch((e) => setErr(e.message));
  }, [f, kind, dl]);

  // Office: make sure the server can produce the PDF before showing the frame.
  React.useEffect(() => {
    if (!f || kind !== "office") return;
    setOffice("loading");
    fetch(preview, { method: "GET", headers: { Range: "bytes=0-0" } })
      .then((r) => setOffice(r.ok || r.status === 206 ? "ok" : "failed"))
      .catch(() => setOffice("failed"));
  }, [f, kind, preview]);

  if (err && !f) return <div className="cx-page"><Note kind="error">{err}</Note></div>;

  return (
    <div className="cx-page" style={{ minHeight: "100vh" }}>
      <div className="cx-crumbs">
        {f && basket && <><Link href={`/b/${f.system_id}`}>{basket.system_type === "personal" ? "My Drive" : basket.name}</Link>
          {f.folder_id && <><span>/</span><Link href={`/b/${f.system_id}?folder=${f.folder_id}`}>{folderName ?? "…"}</Link></>}</>}
      </div>
      <div className="cx-title-row">
        <h1 className="cx-title">{f?.name ?? " "}</h1>
        {f && <StatusTag code={f.status} statuses={settings?.statuses} />}
        <span className="cx-sub cx-num">{f ? `${ext(f.name)} · ${bytes(f.size_bytes)} · version ${f.version}` : ""}</span>
        <span className="cx-spacer" />
        {f && <Link className="cx-btn cx-btn-ghost" href={`/b/${f.system_id}?${f.folder_id ? `folder=${f.folder_id}&` : ""}file=${f.id}`}>Details</Link>}
        <a className="cx-btn cx-btn-primary" href={dl}>Download<I.download /></a>
      </div>
      {f && <div className="cx-sub">Modified <When iso={f.modified_at} /> · {f.owner}</div>}

      <div style={{ background: "var(--cx-layer)", flex: 1, display: "flex", flexDirection: "column", minHeight: 480 }}>
        {!f && <div style={{ padding: 16 }}><span className="cx-skel" style={{ width: "40%" }} /></div>}

        {f && kind === "pdf" && <iframe title={f.name} src={preview} style={frame} />}

        {f && kind === "office" && (
          office === "ok" ? <iframe title={f.name} src={preview} style={frame} />
          : office === "failed" ? <div className="cx-state"><strong>This document can&apos;t be shown here right now.</strong>
              <span>The server couldn&apos;t convert it for viewing. Download it to open it in Office.</span>
              <a className="cx-btn cx-btn-tertiary" href={dl}>Download</a></div>
          : <div style={{ padding: 16 }}><span className="cx-skel" style={{ width: "60%" }} /><span className="cx-skel" style={{ width: "45%", marginTop: 8 }} /></div>
        )}

        {f && kind === "image" && (
          <div style={{ flex: 1, display: "grid", placeItems: "center", padding: 16, background: "var(--cx-layer-2)" }}>
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img src={dl} alt={f.name} style={{ maxWidth: "100%", maxHeight: "75vh", objectFit: "contain" }} />
          </div>
        )}

        {f && kind === "audio" && (
          <div style={{ padding: 24 }}><audio controls preload="metadata" src={dl} style={{ width: "100%" }} /></div>
        )}
        {f && kind === "video" && (
          <div style={{ padding: 16, display: "grid", placeItems: "center" }}><video controls preload="metadata" src={dl} style={{ maxWidth: "100%", maxHeight: "75vh" }} /></div>
        )}

        {f && kind === "html" && (
          <>
            <Tabs value={mode} onChange={setMode} tabs={[{ id: "rendered", label: "Rendered" }, { id: "source", label: "Source" }]} />
            {text == null ? <TooBigOrLoading f={f} dl={dl} />
              : mode === "rendered"
                // sandbox with no permissions: no scripts, no forms, no same-origin access.
                ? <iframe title={f.name} sandbox="" srcDoc={text} style={{ ...frame, background: "#fff" }} />
                : <pre style={pre}>{text}</pre>}
          </>
        )}

        {f && kind === "text" && (text == null ? <TooBigOrLoading f={f} dl={dl} /> : <pre style={pre}>{text}</pre>)}

        {f && kind === "none" && (
          <div className="cx-state"><strong>This file type can&apos;t be shown here.</strong>
            <a className="cx-btn cx-btn-tertiary" href={dl}>Download</a></div>
        )}
      </div>
      {err && f && <Note kind="error">{err}</Note>}
      <div><Btn kind="ghost" onClick={() => router.back()}>Back</Btn></div>
    </div>
  );
}

function TooBigOrLoading({ f, dl }: { f: FileRow; dl: string }) {
  if (f.size_bytes > TEXT_LIMIT) {
    return <div className="cx-state"><strong>This file is too large to show here ({bytes(f.size_bytes)}).</strong><a className="cx-btn cx-btn-tertiary" href={dl}>Download</a></div>;
  }
  return <div style={{ padding: 16 }}><span className="cx-skel" style={{ width: "70%" }} /><span className="cx-skel" style={{ width: "50%", marginTop: 8 }} /></div>;
}

const frame: React.CSSProperties = { flex: 1, width: "100%", minHeight: "75vh", border: 0, display: "block" };
const pre: React.CSSProperties = {
  margin: 0, padding: 16, flex: 1, overflow: "auto", fontFamily: "var(--cx-mono)", fontSize: 12, lineHeight: 1.6,
  whiteSpace: "pre-wrap", overflowWrap: "anywhere", maxHeight: "75vh", color: "var(--cx-text-primary)",
};

/// Indent XML/JSON for reading; falls back to the original text.
function pretty(name: string, t: string): string {
  try {
    if (/\.json$/i.test(name)) return JSON.stringify(JSON.parse(t), null, 2);
    if (!t.includes("\n<") && t.length < 1_000_000) {
      let depth = 0;
      return t.replace(/>\s*</g, ">\n<").split("\n").map((line) => {
        if (/^<\//.test(line)) depth = Math.max(0, depth - 1);
        const out = "  ".repeat(depth) + line;
        if (/^<[^!?/][^>]*[^/]>$/.test(line) && !/<\/[^>]+>$/.test(line)) depth++;
        return out;
      }).join("\n");
    }
  } catch { /* keep original */ }
  return t;
}
