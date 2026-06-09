"use client";

import * as React from "react";
import * as tus from "tus-js-client";

import { Ico } from "@/components/icons";
import { Ft, Pill, SectionHd, Tag } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { useAuth } from "@/lib/auth-context";
import { fmtBytes } from "@/lib/format";
import { canMutate } from "@/lib/roles";
import type { Org, System } from "@/lib/api";

// Files larger than this threshold use TUS resumable upload (PATCH chunks
// with Upload-Offset tracking). Smaller files take the simple multipart path
// because the round-trip overhead of TUS isn't worth it.
const TUS_THRESHOLD_BYTES = 50 * 1024 * 1024;

type Item = {
  file: File;
  // Relative path within a dropped folder (e.g. "reports/q1/summary.pdf").
  // Empty for files picked individually or dropped at the top level — we only
  // surface it in the UI so the user can tell apart same-named files from
  // different folders; the backend still keys on file.name.
  relPath: string;
  progress: number;
  state: "queued" | "uploading" | "success" | "error";
  message?: string;
};

const FT_GUESS: Record<string, string> = {
  pdf: "pdf", doc: "docx", docx: "docx", xls: "xlsx", xlsx: "xlsx",
  ppt: "pptx", pptx: "pptx", html: "html", htm: "html", xml: "xml",
  csv: "csv", json: "json", zip: "zip", txt: "txt", md: "md",
  png: "png", jpg: "img", jpeg: "img", gif: "img",
  wav: "wav", mp3: "mp3",
  mp4: "mp4", mov: "mp4", webm: "mp4",
};

function guessFt(name: string): string {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  return FT_GUESS[ext] ?? "file";
}

// --- Folder drag-and-drop ---------------------------------------------------
// Chromium/WebKit expose a non-standard webkitGetAsEntry() on the items of a
// drop's DataTransfer that lets us walk a dropped directory tree. The DOM lib
// doesn't type these, so we narrow against minimal local shapes.

type FsEntry = {
  isFile: boolean;
  isDirectory: boolean;
  name: string;
  file: (cb: (f: File) => void, err?: (e: unknown) => void) => void;
  createReader: () => { readEntries: (cb: (e: FsEntry[]) => void, err?: (e: unknown) => void) => void };
};

function entryToFile(entry: FsEntry): Promise<File> {
  return new Promise((resolve, reject) => entry.file(resolve, reject));
}

// readEntries returns at most ~100 entries per call, so loop until it drains.
function readAllEntries(reader: ReturnType<FsEntry["createReader"]>): Promise<FsEntry[]> {
  return new Promise((resolve, reject) => {
    const out: FsEntry[] = [];
    const pump = () =>
      reader.readEntries((batch) => {
        if (!batch.length) resolve(out);
        else { out.push(...batch); pump(); }
      }, reject);
    pump();
  });
}

// Depth-first walk of a dropped entry, accumulating {file, relPath}. prefix is
// the path built up from ancestor directory names ("" at the drop root).
async function walkEntry(entry: FsEntry, prefix: string): Promise<{ file: File; relPath: string }[]> {
  if (entry.isFile) {
    try {
      const file = await entryToFile(entry);
      return [{ file, relPath: prefix ? `${prefix}/${entry.name}` : entry.name }];
    } catch {
      return [];
    }
  }
  if (entry.isDirectory) {
    const children = await readAllEntries(entry.createReader());
    const dirPath = prefix ? `${prefix}/${entry.name}` : entry.name;
    const nested = await Promise.all(children.map((c) => walkEntry(c, dirPath)));
    return nested.flat();
  }
  return [];
}

export default function UploadPage() {
  const [items, setItems]       = React.useState<Item[]>([]);
  const [dragging, setDragging] = React.useState(false);
  const [systems, setSystems]   = React.useState<System[]>([]);
  const [orgs, setOrgs]         = React.useState<Org[]>([]);
  const [systemId, setSystemId] = React.useState<string>("");
  const [orgId, setOrgId]       = React.useState<string>("");
  const [project, setProject]   = React.useState("");
  const [status, setStatus]     = React.useState("Draft");
  const [owner, setOwner]       = React.useState("");
  const [tags, setTags]         = React.useState<string[]>([]);
  const inputRef = React.useRef<HTMLInputElement>(null);

  // Default the Owner field to the signed-in user's display name once auth
  // loads. We only seed it if the user hasn't already typed something, so we
  // don't clobber a manual override (e.g. uploading on behalf of someone else).
  const { user: authUser, loading: authLoading } = useAuth();
  // Viewers can't upload (backend require_role(admin|editor) → 403), so show a
  // read-only notice instead of a dropzone that would only fail on submit.
  const readOnly = !canMutate(authUser?.role);
  React.useEffect(() => {
    if (authUser && !owner) setOwner(authUser.display_name);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [authUser]);

  // Bootstrap: load the available systems on mount.
  React.useEffect(() => {
    fetch("/filehub/api/systems").then((r) => r.json()).then((rows: System[]) => {
      setSystems(rows);
      if (rows.length && !systemId) setSystemId(rows[0].id);
    }).catch(() => {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Whenever the chosen system changes, fetch its orgs so the next dropdown
  // only shows valid choices. Clearing org on system change avoids sending an
  // org_id that doesn't belong to the selected system.
  React.useEffect(() => {
    if (!systemId) return;
    setOrgId("");
    fetch(`/filehub/api/orgs?system_id=${encodeURIComponent(systemId)}`)
      .then((r) => r.json())
      .then((rows: Org[]) => setOrgs(rows))
      .catch(() => setOrgs([]));
  }, [systemId]);

  const activeSystem = systems.find((s) => s.id === systemId);

  // Add a batch of {file, relPath} pairs. relPath is the path within a dropped
  // folder; "" for flat file picks. Kept as a separate arg (rather than reading
  // file.webkitRelativePath) because DataTransferItem entries don't populate it.
  const addFiles = (entries: { file: File; relPath: string }[]) => {
    if (!entries.length) return;
    const arr = entries.map<Item>((e) => ({ file: e.file, relPath: e.relPath, progress: 0, state: "queued" }));
    setItems((prev) => [...prev, ...arr]);
  };

  const addPlainFiles = (files: FileList | File[]) =>
    addFiles(Array.from(files).map((f) => ({ file: f, relPath: "" })));

  // Drop handler that supports both files and folders. When the browser exposes
  // webkitGetAsEntry() we walk each entry (recursing into directories) so a
  // dropped folder enqueues all of its files with their relative paths; we fall
  // back to the flat dataTransfer.files list otherwise.
  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setDragging(false);
    const dt = e.dataTransfer;
    const dtItems = dt.items;
    const supportsEntries =
      dtItems && dtItems.length > 0 && typeof (dtItems[0] as unknown as { webkitGetAsEntry?: unknown }).webkitGetAsEntry === "function";

    if (supportsEntries) {
      // Snapshot the entries synchronously — DataTransferItemList is cleared
      // once the drop event handler returns, so we can't await first.
      const entries: FsEntry[] = [];
      for (let i = 0; i < dtItems.length; i++) {
        const entry = (dtItems[i] as unknown as { webkitGetAsEntry: () => FsEntry | null }).webkitGetAsEntry();
        if (entry) entries.push(entry);
      }
      const walked = await Promise.all(entries.map((en) => walkEntry(en, "")));
      addFiles(walked.flat());
      return;
    }

    if (dt.files.length) addPlainFiles(dt.files);
  };

  const uploadOne = (index: number) => {
    const item = items[index];
    if (!item) return;

    const setProgress = (p: number, state: Item["state"], message?: string) => {
      setItems((prev) => prev.map((it, i) => (i === index ? { ...it, progress: p, state, message } : it)));
    };

    // Large files → TUS resumable upload so a dropped connection can pick
    // up where it left off rather than restarting.
    if (item.file.size >= TUS_THRESHOLD_BYTES) {
      // The backend returns a relative Location header (`uploads/<id>`).
      // tus-js-client resolves it against the endpoint URL using `new URL`.
      // If `endpoint` is a path string (`/filehub/api/uploads`) the resolver
      // falls back to `window.location.href` as the base (`/filehub/upload`),
      // which strips the `/api/` segment and produces `/filehub/uploads/<id>`
      // — a path Next.js can't proxy, so every PATCH 404s and the upload
      // hangs at 0 %.  Passing an absolute URL forces the resolver to use
      // the endpoint origin and the relative Location resolves correctly.
      const endpoint = `${window.location.origin}/filehub/api/uploads`;
      // Only include metadata fields the user actually filled in.  TUS
      // metadata is sent as base64-encoded key/value pairs and the backend
      // would have to special-case empty strings; mirroring the multipart
      // path's `if (orgId)` guard keeps the contract simple.
      const meta: Record<string, string> = {
        filename:     item.file.name,
        content_type: item.file.type || "application/octet-stream",
        tags:         JSON.stringify(tags),
      };
      if (systemId) meta.system_id = systemId;
      if (orgId)    meta.org_id    = orgId;
      if (project)  meta.project   = project;
      if (status)   meta.status    = status;
      if (owner)    meta.owner     = owner;

      const upload = new tus.Upload(item.file, {
        endpoint,
        chunkSize: 8 * 1024 * 1024,         // 8 MB PATCH chunks
        retryDelays: [0, 1000, 3000, 5000],
        metadata: meta,
        onError: (err) => setProgress(0, "error", err.message),
        onProgress: (sent, total) => setProgress(Math.round((sent / total) * 100), "uploading"),
        onSuccess: () => setProgress(100, "success", "Done (TUS)"),
      });
      upload.start();
      return;
    }

    // Small files → simple multipart upload.
    const fd = new FormData();
    fd.append("file", item.file);
    if (systemId) fd.append("system_id", systemId);
    if (orgId)    fd.append("org_id",    orgId);
    if (project)  fd.append("project",   project);
    if (status)   fd.append("status",    status);
    if (owner)    fd.append("owner",     owner);
    fd.append("tags", JSON.stringify(tags));

    const xhr = new XMLHttpRequest();
    xhr.open("POST", "/filehub/api/files");
    xhr.withCredentials = true;
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable) setProgress(Math.round((e.loaded / e.total) * 100), "uploading");
    };
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) setProgress(100, "success", "Done");
      else setProgress(0, "error", xhr.responseText || `${xhr.status}`);
    };
    xhr.onerror = () => setProgress(0, "error", "network error");
    xhr.send(fd);
  };

  const uploadAll = () => {
    items.forEach((it, i) => {
      if (it.state === "queued") uploadOne(i);
    });
  };

  // Re-run a single failed item. We flip it back to "queued" (clearing the old
  // error message + progress) before kicking off uploadOne so the row shows
  // the right status immediately even before the first onProgress fires.
  const retryOne = (index: number) => {
    setItems((prev) => prev.map((it, i) => (i === index ? { ...it, progress: 0, state: "queued", message: undefined } : it)));
    uploadOne(index);
  };

  const retryAllFailed = () => {
    items.forEach((it, i) => {
      if (it.state === "error") retryOne(i);
    });
  };

  const queued    = items.filter((it) => it.state === "queued").length;
  const success   = items.filter((it) => it.state === "success").length;
  const uploading = items.filter((it) => it.state === "uploading").length;
  const failed    = items.filter((it) => it.state === "error").length;
  const totalSize = items.reduce((s, it) => s + it.file.size, 0);

  return (
    <div className="scr">
      <Sidebar nav="files" systems={systems} orgs={orgs} systemActive={systemId} />
      <TopBar
        crumbs={["Workspace", "Upload"]}
        title="Upload files"
        actions={
          <>
            <a className="btn ghost" href="/files">Cancel</a>
            {!readOnly && (
              <button className="btn primary" onClick={uploadAll} disabled={queued === 0 || !systemId}>
                Upload {queued || items.length} file{queued === 1 ? "" : "s"}
              </button>
            )}
          </>
        }
      />
      <div className="main main-pad" style={{ overflow: "auto" }}>
        <div className="page">
        {authLoading ? (
          <div style={{ padding: "64px 32px", textAlign: "center", color: "var(--text-subtle)" }}>Loading…</div>
        ) : readOnly ? (
          <ReadOnlyNotice />
        ) : (
        <div className="row-2col" style={{ maxWidth: 1100, margin: "0 auto", padding: "24px 32px" }}>
          <div>
            <SectionHd
              title="Upload files"
              sub="Drop files or whole folders here, or browse. Files are streamed to the Rust backend and encrypted at rest."
            />

            <div
              className={"dropzone" + (dragging ? " dragging" : items.length > 0 ? " active" : "")}
              style={{ marginBottom: 16 }}
              onDragOver={(e) => { e.preventDefault(); setDragging(true); }}
              onDragLeave={() => setDragging(false)}
              onDrop={handleDrop}
              onClick={() => inputRef.current?.click()}
            >
              <input
                ref={inputRef}
                type="file"
                multiple
                aria-label="Select files to upload"
                style={{ display: "none" }}
                onChange={(e) => e.target.files && addPlainFiles(e.target.files)}
              />
              <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 10, pointerEvents: "none" }}>
                <div style={{ width: 56, height: 56, borderRadius: "50%", background: "var(--accent-soft)", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--accent)" }}>
                  <Ico.upload className="icon lg" />
                </div>
                <div className="t-lg t-semibold">Drop files or folders to upload</div>
                <div className="t-sm t-muted">
                  or <span style={{ color: "var(--accent)", textDecoration: "underline" }}>browse from computer</span>
                </div>
              </div>
            </div>

            <div className="card" style={{ padding: 0 }}>
              <div style={{ padding: "12px 14px", display: "flex", alignItems: "center", justifyContent: "space-between", borderBottom: "1px solid var(--border)" }}>
                <div>
                  <div className="t-md t-semibold">
                    Queue · {items.length} file{items.length === 1 ? "" : "s"} · {fmtBytes(totalSize)}
                  </div>
                  <div className="t-xs t-muted">
                    {success} uploaded · {uploading} uploading · {queued} queued{failed > 0 ? ` · ${failed} failed` : ""}
                  </div>
                </div>
                <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                  {failed > 0 && (
                    <button className="btn xs ghost" onClick={retryAllFailed} disabled={!systemId}>
                      <Ico.refresh className="icon sm" /> Retry all failed
                    </button>
                  )}
                  <button className="btn xs ghost" onClick={() => setItems((prev) => prev.filter((it) => it.state !== "success"))}>
                    Clear completed
                  </button>
                  {success > 0 && (
                    <a className="btn xs primary" href={systemId ? `/files?system_id=${encodeURIComponent(systemId)}` : "/files"}>
                      View {success} in Files →
                    </a>
                  )}
                </div>
              </div>
              {items.length === 0 && (
                <div style={{ padding: 28, textAlign: "center", color: "var(--text-subtle)" }}>
                  Drop files in the zone above to add them here.
                </div>
              )}
              {items.map((it, i) => (
                <div key={i} style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 14px", borderTop: "1px solid var(--border-subtle)" }}>
                  <Ft type={guessFt(it.file.name)} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span className="t-sm t-medium t-trunc">{it.file.name}</span>
                      {it.state === "queued"    && <Pill tone="slate"><span className="dot" />Queued</Pill>}
                      {it.state === "uploading" && <Pill tone="indigo"><span className="dot" />Uploading {it.progress}%</Pill>}
                      {it.state === "success"   && <Pill tone="emerald"><span className="dot" />Done</Pill>}
                      {it.state === "error"     && <Pill tone="rose"><span className="dot" />Failed</Pill>}
                    </div>
                    {it.relPath && it.relPath !== it.file.name && (
                      <div className="t-xs t-subtle t-mono t-trunc" style={{ marginTop: 2, display: "flex", alignItems: "center", gap: 4 }}>
                        <Ico.folder className="icon sm" />{it.relPath}
                      </div>
                    )}
                    <div className="t-xs t-muted" style={{ marginTop: 2 }}>
                      {it.message ?? (it.state === "queued" ? "queued" : it.state === "uploading" ? `uploading… ${it.progress}%` : "")}
                    </div>
                    {(it.state === "uploading" || it.state === "queued") && (
                      <div className="prog" style={{ marginTop: 6 }}>
                        <div className="bar" style={{ width: `${it.progress}%` }} />
                      </div>
                    )}
                  </div>
                  <div className="t-xs t-mono t-muted" style={{ width: 70, textAlign: "right" }}>{fmtBytes(it.file.size)}</div>
                  <div className="t-xs t-tabular t-muted" style={{ width: 40, textAlign: "right" }}>{it.progress}%</div>
                  {it.state === "error" && (
                    <button
                      className="btn xs ghost icon"
                      title="Retry this upload"
                      aria-label="Retry this upload"
                      disabled={!systemId}
                      onClick={() => retryOne(i)}
                    >
                      <Ico.refresh className="icon sm" />
                    </button>
                  )}
                  <button
                    className="btn xs ghost icon"
                    title="Remove from queue"
                    aria-label="Remove from queue"
                    onClick={() => setItems((prev) => prev.filter((_, j) => j !== i))}
                  >
                    <Ico.x className="icon sm" />
                  </button>
                </div>
              ))}
            </div>
          </div>

          <div>
            <div className="card" style={{ padding: 16, marginBottom: 16 }}>
              <SectionHd title="Destination" sub="System + org where these files will live" />
              <Field label="System" htmlFor="system-select">
                {activeSystem && <Pill tone={activeSystem.tone}><span className="dot" />{activeSystem.name}</Pill>}
                <select
                  id="system-select"
                  value={systemId}
                  onChange={(e) => setSystemId(e.target.value)}
                  style={{ flex: 1, border: 0, background: "transparent", color: "inherit", font: "inherit" }}
                >
                  {!systems.length && <option value="">(loading systems…)</option>}
                  {systems.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
                </select>
              </Field>
              <Field label="Org (optional)" htmlFor="org-select">
                <select
                  id="org-select"
                  value={orgId}
                  onChange={(e) => setOrgId(e.target.value)}
                  style={{ flex: 1, border: 0, background: "transparent", color: "inherit", font: "inherit" }}
                  disabled={!orgs.length}
                >
                  <option value="">{orgs.length ? "— none —" : "(no orgs in this system)"}</option>
                  {orgs.map((o) => <option key={o.id} value={o.id}>{o.name} ({o.code})</option>)}
                </select>
              </Field>
              <div className="t-xs t-muted" style={{ marginTop: 8, display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                <span>Bucket:</span>
                <span className="t-mono">{activeSystem?.bucket ?? "—"}</span>
                <span className="t-subtle">local filesystem · encrypted at rest</span>
              </div>
            </div>

            <div className="card" style={{ padding: 16, marginBottom: 16 }}>
              <SectionHd
                title="Apply metadata to all"
                sub="Fields are sent with every upload"
                action={<Pill tone="indigo">{items.length} file{items.length === 1 ? "" : "s"}</Pill>}
              />
              <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                <Field label="Project" htmlFor="project-input">
                  <input
                    id="project-input"
                    value={project}
                    onChange={(e) => setProject(e.target.value)}
                    placeholder="e.g. Q1-2026"
                    style={{ width: "100%" }}
                  />
                </Field>
                <Field label="Status" htmlFor="status-select">
                  <select
                    id="status-select"
                    value={status}
                    onChange={(e) => setStatus(e.target.value)}
                    style={{ width: "100%", border: 0, background: "transparent", color: "inherit", font: "inherit" }}
                  >
                    {["Draft", "Review", "Approved", "Archived"].map((s) => <option key={s}>{s}</option>)}
                  </select>
                </Field>
                <Field label="Owner" htmlFor="owner-input">
                  <input
                    id="owner-input"
                    value={owner}
                    onChange={(e) => setOwner(e.target.value)}
                    placeholder="Your name"
                    style={{ width: "100%" }}
                  />
                </Field>
                <Field label="Tags" multiline htmlFor="tags-input">
                  {tags.map((t) => (
                    <span key={t} style={{ display: "inline-flex", alignItems: "center" }}>
                      <Tag>{t}</Tag>
                      <button
                        type="button"
                        className="btn xs ghost icon"
                        onClick={() => setTags((prev) => prev.filter((x) => x !== t))}
                      ><Ico.x className="icon sm" /></button>
                    </span>
                  ))}
                  <input
                    id="tags-input"
                    placeholder="Add tag + Enter"
                    style={{ minWidth: 100 }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && e.currentTarget.value) {
                        e.preventDefault();
                        setTags((prev) => Array.from(new Set([...prev, e.currentTarget.value])));
                        e.currentTarget.value = "";
                      }
                    }}
                  />
                </Field>
              </div>
            </div>
          </div>
        </div>
        )}
        </div>
      </div>
    </div>
  );
}

function ReadOnlyNotice() {
  return (
    <div style={{ maxWidth: 520, margin: "48px auto", padding: "0 32px", textAlign: "center" }}>
      <div style={{ width: 56, height: 56, borderRadius: "50%", background: "var(--bg-strong)", color: "var(--text-muted)", display: "flex", alignItems: "center", justifyContent: "center", margin: "0 auto 16px" }}>
        <Ico.lock className="icon lg" />
      </div>
      <div className="t-lg t-semibold">Uploading needs editor access</div>
      <div className="t-sm t-muted" style={{ marginTop: 6 }}>
        Your role is <span className="t-semibold">viewer</span> — read-only. You can browse, download, and comment on
        files, but uploading is limited to editors and admins. Ask an admin to change your role if you need to upload.
      </div>
      <div style={{ marginTop: 20 }}>
        <a className="btn primary" href="/files">Back to files</a>
      </div>
    </div>
  );
}

function Field({ label, children, multiline, htmlFor }: { label: string; children: React.ReactNode; multiline?: boolean; htmlFor?: string }) {
  return (
    <div style={{ marginBottom: 10 }}>
      <label htmlFor={htmlFor} className="t-xs t-subtle t-medium" style={{ display: "block", marginBottom: 4 }}>{label}</label>
      <div className="field" style={{
        width: "100%",
        height: multiline ? "auto" : undefined,
        minHeight: multiline ? 30 : undefined,
        padding: multiline ? "4px 8px" : undefined,
        flexWrap: multiline ? "wrap" : undefined,
      }}>
        {children}
      </div>
    </div>
  );
}
