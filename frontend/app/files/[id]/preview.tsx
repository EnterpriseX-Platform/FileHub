"use client";

import * as React from "react";

import { Ft } from "@/components/primitives";

const OFFICE_TYPES = new Set(["docx", "doc", "xlsx", "xls", "pptx", "ppt", "odt", "ods", "odp"]);
const TEXT_TYPES   = new Set(["txt", "md", "csv", "log", "json", "xml"]);

type OfficeUrl = { iframe_url: string; viewer: string; mode: string };

// First 64 KB of a text file, rendered in a <pre>. Uses a Range request (the
// backend supports single ranges) so a 2 GB log file costs one small read;
// a 206 response means there's more than we show.
const TEXT_PREVIEW_BYTES = 64 * 1024;

function TextPreview({ download, fileName }: { download: string; fileName: string }) {
  const [state, setState] = React.useState<{ text: string; truncated: boolean } | { error: string } | null>(null);

  React.useEffect(() => {
    let alive = true;
    fetch(download, {
      credentials: "include",
      headers: { Range: `bytes=0-${TEXT_PREVIEW_BYTES - 1}` },
    })
      .then(async (r) => {
        if (!r.ok && r.status !== 206) throw new Error(`HTTP ${r.status}`);
        const text = await r.text();
        if (alive) setState({ text, truncated: r.status === 206 });
      })
      .catch((e) => { if (alive) setState({ error: e instanceof Error ? e.message : String(e) }); });
    return () => { alive = false; };
  }, [download]);

  if (state && "error" in state) {
    return <div className="t-sm t-muted" style={{ padding: 24 }}>Couldn&apos;t load preview ({state.error}) — <a href={download} style={{ color: "var(--accent)" }}>download instead</a>.</div>;
  }

  return (
    <div style={{ width: "min(720px, 100%)", background: "var(--bg)", border: "1px solid var(--border)", borderRadius: "var(--r-3)", boxShadow: "var(--sh-1)", overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 14px", borderBottom: "1px solid var(--border)" }}>
        <span className="t-sm t-semibold t-trunc" style={{ flex: 1 }}>{fileName}</span>
        {state && !("error" in state) && state.truncated && <span className="t-xs t-subtle">first 64 KB</span>}
        <a href={download} className="btn xs ghost">Download</a>
      </div>
      <pre className="t-mono t-sm" style={{ margin: 0, padding: 16, maxHeight: "62vh", overflow: "auto", whiteSpace: "pre-wrap", wordBreak: "break-word", color: "var(--text)" }}>
        {state === null ? "Loading…" : state.text || "(empty file)"}
      </pre>
    </div>
  );
}

/// In-place preview of the file. PDFs and images go straight to the browser.
/// Office docs use the server-rendered PDF preview at /api/files/:id/preview
/// (LibreOffice converts on upload, see backend/src/p1.rs).
export function FilePreview({ fileId, fileType, fileName }: { fileId: string; fileType: string; fileName: string }) {
  const [imgError, setImgError] = React.useState(false);
  const download = `/filehub/api/files/${encodeURIComponent(fileId)}/download`;
  const preview  = `/filehub/api/files/${encodeURIComponent(fileId)}/preview`;

  // PDFs and Office docs both render as an `<iframe>` pointing at a PDF
  // source.  We used to use `<object>` here, but Chrome's PDF plugin
  // inside an `<object>` element silently rendered as a black box when
  // the OS was set to dark mode (the plugin's "auto" theme decided to
  // invert).  `<iframe>` triggers the full PDF viewer with its own light
  // background, so the document is always readable regardless of the
  // user's OS theme.
  // For PDFs we use the preview endpoint, NOT the download endpoint.
  // `download` sends `Content-Disposition: attachment` which causes Chrome
  // to start a file download instead of rendering inline — `<iframe>` then
  // shows as a blank white box.  The preview endpoint returns the same
  // bytes with `Content-Type: application/pdf` and no attachment header.
  if (fileType === "pdf") {
    return (
      <iframe
        src={preview}
        title={fileName}
        style={{ width: "min(720px, 100%)", height: "70vh", border: 0, borderRadius: 4, background: "var(--bg)", boxShadow: "var(--sh-3)", colorScheme: "light" }}
      />
    );
  }

  if (OFFICE_TYPES.has(fileType)) {
    return <OfficePreview fileId={fileId} fileType={fileType} fileName={fileName} download={download} preview={preview} />;
  }

  if ((fileType === "img" || fileType === "png" || fileType === "jpg" || fileType === "jpeg") && !imgError) {
    return (
      // next/image is wrong here: the optimizer refetches server-side without
      // the session cookie, so the authenticated download endpoint would 401.
      // eslint-disable-next-line @next/next/no-img-element
      <img
        src={download}
        alt={fileName}
        onError={() => setImgError(true)}
        style={{ maxWidth: "min(720px, 100%)", maxHeight: "70vh", borderRadius: 4, background: "var(--bg)", boxShadow: "var(--sh-3)" }}
      />
    );
  }

  // Video — relies on the backend's `Range` request support added in T1.
  // Without 206 responses, large videos block until the whole file
  // downloads + seek bar can't scrub.
  if (fileType === "mp4" || fileType === "mov" || fileType === "webm") {
    return (
      <video
        src={download}
        controls
        preload="metadata"
        style={{ width: "min(720px, 100%)", maxHeight: "70vh", borderRadius: 4, background: "#000", boxShadow: "var(--sh-3)" }}
      >
        <a href={download} className="btn">Download {fileName}</a>
      </video>
    );
  }

  // Plain text — fetch the first 64 KB inline instead of wasting the whole
  // preview pane on a "no preview" card.
  if (TEXT_TYPES.has(fileType)) {
    return <TextPreview download={download} fileName={fileName} />;
  }

  // Audio — same Range-based streaming path.
  if (fileType === "mp3" || fileType === "wav") {
    return (
      <div style={{ width: "min(540px, 100%)", padding: 24, background: "var(--bg)", border: "1px solid var(--border)", borderRadius: "var(--r-3)", boxShadow: "var(--sh-1)" }}>
        <div className="t-base t-semibold t-trunc" style={{ marginBottom: 12 }}>{fileName}</div>
        <audio src={download} controls preload="metadata" style={{ width: "100%" }}>
          <a href={download} className="btn">Download {fileName}</a>
        </audio>
      </div>
    );
  }

  return (
    <div style={{
      width: "min(540px, 100%)", padding: 48,
      background: "var(--bg)", border: "1px solid var(--border)",
      borderRadius: "var(--r-3)", boxShadow: "var(--sh-1)",
      display: "flex", flexDirection: "column", alignItems: "center", gap: 12, color: "var(--text-muted)",
    }}>
      <Ft type={fileType} size="xl" />
      <div className="t-base t-semibold t-trunc" style={{ color: "var(--text)" }}>{fileName}</div>
      <div className="t-sm t-muted">No inline preview for .{fileType}</div>
      <a href={download} className="btn primary">Download to view</a>
    </div>
  );
}

/// Collabora iframe with File Hub branding overrides applied at runtime.
///
/// Collabora's PostMessage API lets us push CSS into the editor without
/// rebuilding the container image: as soon as the editor signals it's
/// ready (`App_LoadingStatus = Document_Loaded`), the parent posts an
/// `Host_PostmessageReady` ack and then `Action_LoadStyleSheet` with a
/// URL pointing at `/filehub/branding/collabora.css`.  That CSS hides
/// the "Explore the New" welcome dialog and re-tints the toolbar to the
/// File Hub indigo — without touching the underlying Collabora image.
function CollaboraFrame({ iframeUrl, fileName, fileType, mode, download }: {
  iframeUrl: string; fileName: string; fileType: string; mode: string; download: string;
}) {
  const ref = React.useRef<HTMLIFrameElement>(null);

  React.useEffect(() => {
    const onMessage = (ev: MessageEvent) => {
      // Collabora posts JSON-stringified payloads.  We only care about
      // the document-loaded signal so we know it's safe to push CSS.
      let msg: { MessageId?: string; Values?: unknown } | null = null;
      try { msg = typeof ev.data === "string" ? JSON.parse(ev.data) : (ev.data as typeof msg); }
      catch { return; }
      if (!msg?.MessageId) return;

      const send = (m: object) => ref.current?.contentWindow?.postMessage(JSON.stringify(m), "*");

      if (msg.MessageId === "App_LoadingStatus") {
        // Acknowledge Collabora's loading status so it knows it's
        // embedded by a friendly host.
        send({ MessageId: "Host_PostmessageReady" });
      }
      if (msg.MessageId === "Doc_ModifiedStatus" || msg.MessageId === "App_LoadingStatus") {
        // Inject our branding CSS once the document is open.  Cheap
        // enough to send on every status tick — Collabora dedupes.
        send({
          MessageId: "Action_LoadStyleSheet",
          Values: { url: `${window.location.origin}/filehub/branding/collabora.css` },
        });
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  return (
    <div style={{ width: "min(960px, 100%)" }}>
      <iframe
        ref={ref}
        src={iframeUrl}
        title={fileName}
        allow="clipboard-read; clipboard-write"
        style={{ width: "100%", height: "78vh", border: 0, borderRadius: 4, background: "var(--bg)", boxShadow: "var(--sh-3)" }}
      />
      <div className="t-xs t-subtle" style={{ marginTop: 6, textAlign: "center" }}>
        File Hub Editor · {mode === "edit" ? "edit mode" : "view-only"} · <a href={download} style={{ color: "var(--accent)" }}>download .{fileType}</a>
      </div>
    </div>
  );
}

/// Office preview that picks between Collabora and PDF based on
/// `/api/files/:id/office-url` — the backend reads
/// `workspace_config.office_viewer` and decides for us.  Fallback chain:
///   1. iframe to Collabora (when viewer = "collabora", WOPI URL returned)
///   2. PDF preview iframe (when viewer = "pdf" or Collabora request fails)
///   3. Manual download card (when viewer = "disabled")
function OfficePreview({ fileId, fileType, fileName, download, preview }: {
  fileId: string; fileType: string; fileName: string; download: string; preview: string;
}) {
  const [info, setInfo] = React.useState<OfficeUrl | null>(null);
  const [err,  setErr]  = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/office-url`, {
          credentials: "include", cache: "no-store",
        });
        if (!cancelled) {
          if (r.ok) setInfo(await r.json());
          else      setErr(`HTTP ${r.status}`);
        }
      } catch (e) { if (!cancelled) setErr(String(e)); }
    })();
    return () => { cancelled = true; };
  }, [fileId]);

  // While we're waiting for the viewer choice, show a skeleton so the page
  // doesn't flash an empty box.
  if (info === null && err === null) {
    return (
      <div style={{ width: "min(720px, 100%)", height: "70vh", background: "var(--bg-subtle)", borderRadius: 4 }} />
    );
  }

  // Collabora path — full WebSocket-backed editor in an iframe.
  if (info?.viewer === "collabora" && info.iframe_url) {
    return <CollaboraFrame iframeUrl={info.iframe_url} fileName={fileName} fileType={fileType} mode={info.mode} download={download} />;
  }

  // disabled path — surface a clear download card so the user knows
  // why no preview is showing.
  if (info?.viewer === "disabled") {
    return (
      <div style={{ width: "min(540px, 100%)", padding: 32, background: "var(--bg)", border: "1px solid var(--border)", borderRadius: "var(--r-3)", boxShadow: "var(--sh-1)" }}>
        <div className="t-base t-semibold" style={{ marginBottom: 8 }}>{fileName}</div>
        <div className="t-sm t-muted" style={{ marginBottom: 12 }}>Inline office preview is disabled in workspace settings.</div>
        <a href={download} className="btn primary">Download .{fileType}</a>
      </div>
    );
  }

  // Default — server-rendered PDF preview iframe.  Same path the page
  // used before Collabora landed.
  return (
    <div style={{ width: "min(720px, 100%)" }}>
      <iframe
        src={preview}
        title={fileName}
        style={{ width: "100%", height: "70vh", border: 0, borderRadius: 4, background: "var(--bg)", boxShadow: "var(--sh-3)", colorScheme: "light" }}
      />
      <div className="t-xs t-subtle" style={{ marginTop: 6, textAlign: "center" }}>
        Server-rendered PDF preview · <a href={download} style={{ color: "var(--accent)" }}>download .{fileType}</a>
      </div>
    </div>
  );
}
