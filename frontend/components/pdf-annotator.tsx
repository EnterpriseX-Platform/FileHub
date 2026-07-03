"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { useAuth } from "@/lib/auth-context";
import { useI18n } from "@/lib/i18n";

/// pdf.js viewer with an annotation overlay — replaces the Chrome-plugin
/// iframe so documents support notes, highlights, and signature stamps
/// (TOR ANNEX-3). Coordinates are stored normalized (0..1) per page, so they
/// are independent of render scale.
///
/// Tools: browse (default), note (click to pin a comment), highlight (drag a
/// rectangle), stamp (place a mark from the user's signature library).

type Annotation = {
  id: string;
  file_id: string;
  page: number;
  x: number;
  y: number;
  w: number;
  h: number;
  kind: "note" | "highlight" | "stamp";
  body: string | null;
  signature_id: string | null;
  signature_image: string | null;
  created_by: string | null;
  author: string | null;
  created_at: string;
};
type SignatureMark = { id: string; label: string; kind: string; image: string };
type Tool = "browse" | "note" | "highlight" | "stamp";

// pdf.js is heavyweight — load it once, lazily, on the client only.
let pdfjsPromise: Promise<typeof import("pdfjs-dist")> | null = null;
function loadPdfjs() {
  if (!pdfjsPromise) {
    pdfjsPromise = import("pdfjs-dist").then((m) => {
      m.GlobalWorkerOptions.workerSrc = "/filehub/pdf.worker.min.mjs";
      return m;
    });
  }
  return pdfjsPromise;
}

export function PdfAnnotator({ fileId, src, fileName }: { fileId: string; src: string; fileName: string }) {
  const { t } = useI18n();
  const { user } = useAuth();
  const containerRef = React.useRef<HTMLDivElement>(null);
  const [pdf, setPdf] = React.useState<import("pdfjs-dist").PDFDocumentProxy | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [zoom, setZoom] = React.useState(1);
  const [tool, setTool] = React.useState<Tool>("browse");
  const [anns, setAnns] = React.useState<Annotation[]>([]);
  const [marks, setMarks] = React.useState<SignatureMark[] | null>(null);
  const [markId, setMarkId] = React.useState<string | null>(null);

  // Document bytes → pdf.js proxy. Cleanup destroys via the loading task
  // (the typed teardown surface — it also frees the document proxy).
  React.useEffect(() => {
    let cancelled = false;
    let task: import("pdfjs-dist").PDFDocumentLoadingTask | null = null;
    (async () => {
      try {
        const pdfjs = await loadPdfjs();
        const r = await fetch(src, { credentials: "include" });
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        const data = await r.arrayBuffer();
        task = pdfjs.getDocument({ data });
        const doc = await task.promise;
        if (!cancelled) setPdf(doc);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
      task?.destroy().catch(() => {});
    };
  }, [src]);

  // Annotations for the file.
  const loadAnns = React.useCallback(async () => {
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/annotations`, {
        credentials: "include",
        cache: "no-store",
      });
      if (r.ok) setAnns(await r.json());
    } catch { /* viewer still works without annotations */ }
  }, [fileId]);
  React.useEffect(() => { loadAnns(); }, [loadAnns]);

  // Signature marks, loaded when the stamp tool is first picked.
  React.useEffect(() => {
    if (tool !== "stamp" || marks !== null) return;
    fetch("/filehub/api/signatures", { credentials: "include", cache: "no-store" })
      .then((r) => (r.ok ? r.json() : []))
      .then((rows: SignatureMark[]) => {
        setMarks(rows);
        setMarkId((cur) => cur ?? rows[0]?.id ?? null);
      })
      .catch(() => setMarks([]));
  }, [tool, marks]);

  const createAnn = async (a: { page: number; x: number; y: number; w: number; h: number; kind: string; body?: string | null; signature_id?: string | null }) => {
    const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/annotations`, {
      method: "POST",
      credentials: "include",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(a),
    });
    if (r.ok) {
      const created = (await r.json()) as Annotation;
      setAnns((s) => [...s, created]);
    }
  };

  const deleteAnn = async (id: string) => {
    const r = await fetch(`/filehub/api/annotations/${encodeURIComponent(id)}`, {
      method: "DELETE",
      credentials: "include",
    });
    if (r.ok) setAnns((s) => s.filter((a) => a.id !== id));
  };

  const canDelete = (a: Annotation) => a.created_by === user?.id || user?.role === "admin";

  if (error) {
    return (
      <div style={{ padding: 32, textAlign: "center", color: "var(--text-muted)" }}>
        <div className="t-sm">Couldn&apos;t render {fileName} ({error})</div>
      </div>
    );
  }

  return (
    <div className="pdfann">
      <div className="pdfann-bar">
        <ToolBtn active={tool === "browse"} onClick={() => setTool("browse")} icon={<Ico.eye className="icon sm" />} label={t("ann.browse")} />
        <ToolBtn active={tool === "note"} onClick={() => setTool("note")} icon={<Ico.pin className="icon sm" />} label={t("ann.note")} />
        <ToolBtn active={tool === "highlight"} onClick={() => setTool("highlight")} icon={<Ico.tag className="icon sm" />} label={t("ann.highlight")} />
        <ToolBtn active={tool === "stamp"} onClick={() => setTool("stamp")} icon={<Ico.check className="icon sm" />} label={t("ann.stamp")} />
        {tool === "stamp" && (
          <div style={{ display: "flex", gap: 4, alignItems: "center", marginLeft: 4 }}>
            {(marks ?? []).map((m) => (
              <button
                key={m.id}
                type="button"
                title={m.label}
                onClick={() => setMarkId(m.id)}
                style={{
                  padding: 1, borderRadius: 6, cursor: "pointer", background: "#fff",
                  border: markId === m.id ? "2px solid var(--accent)" : "1px solid var(--border)",
                }}
              >
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img src={m.image} alt={m.label} style={{ height: 20, maxWidth: 70, display: "block", objectFit: "contain" }} />
              </button>
            ))}
            {marks !== null && marks.length === 0 && (
              <span className="t-xs t-subtle">{t("ann.noMarks")}</span>
            )}
          </div>
        )}
        <div style={{ flex: 1 }} />
        <button className="btn xs ghost" aria-label="Zoom out" onClick={() => setZoom((z) => Math.max(0.5, +(z - 0.15).toFixed(2)))}>−</button>
        <span className="t-xs t-subtle" style={{ minWidth: 40, textAlign: "center" }}>{Math.round(zoom * 100)}%</span>
        <button className="btn xs ghost" aria-label="Zoom in" onClick={() => setZoom((z) => Math.min(2.5, +(z + 0.15).toFixed(2)))}>+</button>
        {pdf && <span className="t-xs t-subtle" style={{ marginLeft: 8 }}>{pdf.numPages} {t("ann.pages")}</span>}
      </div>

      <div ref={containerRef} className="pdfann-scroll">
        {!pdf ? (
          <div className="ai-sk" style={{ height: "60vh", margin: 16, borderRadius: 8 }} />
        ) : (
          Array.from({ length: pdf.numPages }, (_, i) => (
            <PdfPage
              key={i + 1}
              pdf={pdf}
              pageNo={i + 1}
              zoom={zoom}
              tool={tool}
              markId={markId}
              markImage={marks?.find((m) => m.id === markId)?.image ?? null}
              anns={anns.filter((a) => a.page === i + 1)}
              onCreate={createAnn}
              onDelete={deleteAnn}
              canDelete={canDelete}
              afterAct={() => setTool("browse")}
              t={t}
            />
          ))
        )}
      </div>
    </div>
  );
}

function ToolBtn({ active, onClick, icon, label }: { active: boolean; onClick: () => void; icon: React.ReactNode; label: string }) {
  return (
    <button type="button" className={"btn xs" + (active ? " primary" : " ghost")} onClick={onClick}>
      {icon} {label}
    </button>
  );
}

type PendingNote = { x: number; y: number; text: string };
type DragRect = { x0: number; y0: number; x1: number; y1: number };

function PdfPage({ pdf, pageNo, zoom, tool, markId, markImage, anns, onCreate, onDelete, canDelete, afterAct, t }: {
  pdf: import("pdfjs-dist").PDFDocumentProxy;
  pageNo: number;
  zoom: number;
  tool: Tool;
  markId: string | null;
  markImage: string | null;
  anns: Annotation[];
  onCreate: (a: { page: number; x: number; y: number; w: number; h: number; kind: string; body?: string | null; signature_id?: string | null }) => Promise<void>;
  onDelete: (id: string) => void;
  canDelete: (a: Annotation) => boolean;
  afterAct: () => void;
  t: (k: string) => string;
}) {
  const canvasRef = React.useRef<HTMLCanvasElement>(null);
  const wrapRef = React.useRef<HTMLDivElement>(null);
  const [note, setNote] = React.useState<PendingNote | null>(null);
  const [drag, setDrag] = React.useState<DragRect | null>(null);

  // Render the page. Re-runs on zoom; pdf.js render tasks are cancelled on
  // cleanup so fast zooming doesn't race the canvas.
  React.useEffect(() => {
    let cancelled = false;
    let task: { cancel: () => void; promise: Promise<unknown> } | null = null;
    (async () => {
      const page = await pdf.getPage(pageNo);
      if (cancelled) return;
      const canvas = canvasRef.current;
      if (!canvas) return;
      const base = page.getViewport({ scale: 1 });
      // Fit ~860 CSS px wide at 100%, then apply zoom; render at DPR for crispness.
      const cssScale = (860 / base.width) * zoom;
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const viewport = page.getViewport({ scale: cssScale * dpr });
      canvas.width = viewport.width;
      canvas.height = viewport.height;
      canvas.style.width = `${viewport.width / dpr}px`;
      canvas.style.height = `${viewport.height / dpr}px`;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      task = page.render({ canvas, canvasContext: ctx, viewport });
      await task.promise.catch(() => {});
    })();
    return () => { cancelled = true; task?.cancel(); };
  }, [pdf, pageNo, zoom]);

  const norm = (e: React.MouseEvent): { x: number; y: number } => {
    const r = wrapRef.current!.getBoundingClientRect();
    return {
      x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
    };
  };

  const onClick = (e: React.MouseEvent) => {
    if (tool === "note") {
      const p = norm(e);
      setNote({ ...p, text: "" });
    } else if (tool === "stamp" && markId) {
      const p = norm(e);
      // 18% of page width, 7% tall — a sensible default stamp footprint.
      onCreate({ page: pageNo, x: Math.min(p.x, 0.82), y: Math.min(p.y, 0.93), w: 0.18, h: 0.07, kind: "stamp", signature_id: markId });
      afterAct();
    }
  };

  const onMouseDown = (e: React.MouseEvent) => {
    if (tool !== "highlight") return;
    const p = norm(e);
    setDrag({ x0: p.x, y0: p.y, x1: p.x, y1: p.y });
  };
  const onMouseMove = (e: React.MouseEvent) => {
    if (tool !== "highlight" || !drag) return;
    const p = norm(e);
    setDrag((d) => (d ? { ...d, x1: p.x, y1: p.y } : d));
  };
  const onMouseUp = () => {
    if (tool !== "highlight" || !drag) return;
    const x = Math.min(drag.x0, drag.x1);
    const y = Math.min(drag.y0, drag.y1);
    const w = Math.abs(drag.x1 - drag.x0);
    const h = Math.abs(drag.y1 - drag.y0);
    setDrag(null);
    if (w > 0.005 && h > 0.005) {
      onCreate({ page: pageNo, x, y, w, h, kind: "highlight" });
      afterAct();
    }
  };

  const saveNote = async () => {
    if (!note || !note.text.trim()) { setNote(null); return; }
    await onCreate({ page: pageNo, x: note.x, y: note.y, w: 0, h: 0, kind: "note", body: note.text.trim() });
    setNote(null);
    afterAct();
  };

  return (
    <div
      ref={wrapRef}
      className="pdfann-page"
      style={{ cursor: tool === "browse" ? "default" : "crosshair" }}
      onClick={onClick}
      onMouseDown={onMouseDown}
      onMouseMove={onMouseMove}
      onMouseUp={onMouseUp}
    >
      <canvas ref={canvasRef} />

      {/* Saved annotations. */}
      {anns.map((a) =>
        a.kind === "highlight" ? (
          <div key={a.id} className="pdfann-hl" style={{ left: pct(a.x), top: pct(a.y), width: pct(a.w), height: pct(a.h) }}>
            {canDelete(a) && <DeleteX onDelete={() => onDelete(a.id)} />}
          </div>
        ) : a.kind === "stamp" ? (
          <div key={a.id} className="pdfann-stamp" style={{ left: pct(a.x), top: pct(a.y), width: pct(a.w), height: pct(a.h) }}>
            {a.signature_image && (
              // eslint-disable-next-line @next/next/no-img-element
              <img src={a.signature_image} alt={a.body ?? "stamp"} />
            )}
            {canDelete(a) && <DeleteX onDelete={() => onDelete(a.id)} />}
          </div>
        ) : (
          <div key={a.id} className="pdfann-note" style={{ left: pct(a.x), top: pct(a.y) }}>
            <span className="pin"><Ico.pin className="icon sm" /></span>
            <span className="tip">
              <b>{a.author ?? "—"}</b> {a.body}
              {canDelete(a) && (
                <button type="button" className="del" aria-label="Delete note" onClick={(e) => { e.stopPropagation(); onDelete(a.id); }}>✕</button>
              )}
            </span>
          </div>
        ),
      )}

      {/* In-flight highlight drag. */}
      {drag && (
        <div
          className="pdfann-hl dragging"
          style={{
            left: pct(Math.min(drag.x0, drag.x1)),
            top: pct(Math.min(drag.y0, drag.y1)),
            width: pct(Math.abs(drag.x1 - drag.x0)),
            height: pct(Math.abs(drag.y1 - drag.y0)),
          }}
        />
      )}

      {/* Note composer. */}
      {note && (
        <div className="pdfann-composer" style={{ left: pct(note.x), top: pct(note.y) }} onClick={(e) => e.stopPropagation()}>
          <input
            autoFocus
            value={note.text}
            placeholder={t("ann.notePh")}
            onChange={(e) => setNote((n) => (n ? { ...n, text: e.target.value } : n))}
            onKeyDown={(e) => {
              if (e.key === "Enter") saveNote();
              if (e.key === "Escape") setNote(null);
            }}
          />
          <button type="button" className="btn xs primary" onClick={saveNote}>{t("ann.save")}</button>
        </div>
      )}

      {/* Ghost preview of the stamp footprint under the cursor tool hint. */}
      {tool === "stamp" && markImage && (
        <div className="pdfann-stamphint t-xs t-subtle">{t("ann.stampHint")}</div>
      )}
    </div>
  );
}

function DeleteX({ onDelete }: { onDelete: () => void }) {
  return (
    <button type="button" className="del" aria-label="Delete annotation" onClick={(e) => { e.stopPropagation(); onDelete(); }}>✕</button>
  );
}

function pct(v: number) {
  return `${v * 100}%`;
}
