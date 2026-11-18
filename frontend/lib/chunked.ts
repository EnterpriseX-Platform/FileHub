// Chunked, resumable upload (TUS 1.0.0) used by every upload in the app.
//
// - Sends 8 MB chunks; a small file is a single chunk.
// - PATCH/HEAD go as POST + X-HTTP-Method-Override (the edge only lets GET/POST through).
// - A dropped connection asks the server where it got to (HEAD) and carries on,
//   up to 5 times, waiting a little longer each time.
// - Basket rules are checked by the server when the upload starts, so a refused
//   file fails before any byte is sent.

import { BASE_PATH, messageFor } from "./fh";

export const CHUNK_BYTES = 8 * 1024 * 1024;

// Where file bytes go: the deployment may point uploads at a separate host
// (UPLOAD_BASE_URL on the server, e.g. https://upload.example.go.th/filehub).
// Empty = this site. Read once from the public branding endpoint.
let uploadBase: Promise<string> | null = null;
function getUploadBase(): Promise<string> {
  uploadBase ??= fetch(`${BASE_PATH}/api/branding`, { cache: "no-store" })
    .then((r) => (r.ok ? r.json() : {}))
    .then((j: { upload_base?: string }) => (typeof j.upload_base === "string" ? j.upload_base.replace(/\/+$/, "") : ""))
    .catch(() => "");
  return uploadBase;
}

export type ChunkOpts = {
  /** "/api/uploads" (signed in) or "/api/upload/<token>/chunks" (upload link). */
  endpoint: string;
  file: File;
  meta: Record<string, string | undefined | null>;
  onProgress?: (sentBytes: number, total: number) => void;
  signal?: AbortSignal;
};

export class UploadError extends Error {
  constructor(message: string, public status: number) { super(message); }
}

const b64 = (s: string) => btoa(String.fromCharCode(...new TextEncoder().encode(s)));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function req(method: string, url: string, headers: Record<string, string>, body: Blob | null, onSent?: (n: number) => void, signal?: AbortSignal) {
  return new Promise<{ status: number; headers: (k: string) => string | null; text: string }>((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    const real = method.toUpperCase();
    xhr.open(real === "PATCH" || real === "HEAD" || real === "DELETE" ? "POST" : real, url);
    if (/^https?:/i.test(url)) xhr.withCredentials = true; // separate upload host: send the session cookie
    if (real !== "POST") xhr.setRequestHeader("X-HTTP-Method-Override", real);
    xhr.setRequestHeader("Tus-Resumable", "1.0.0");
    for (const [k, v] of Object.entries(headers)) xhr.setRequestHeader(k, v);
    if (onSent) xhr.upload.onprogress = (e) => onSent(e.loaded);
    xhr.onload = () => resolve({ status: xhr.status, headers: (k) => xhr.getResponseHeader(k), text: xhr.responseText });
    xhr.onerror = () => reject(new Error("network"));
    xhr.ontimeout = () => reject(new Error("network"));
    if (signal) {
      if (signal.aborted) { reject(new DOMException("Aborted", "AbortError")); return; }
      signal.addEventListener("abort", () => { xhr.abort(); reject(new DOMException("Aborted", "AbortError")); }, { once: true });
    }
    xhr.send(body);
  });
}

function errorOf(r: { status: number; text: string }) {
  let body: unknown = null;
  try { body = JSON.parse(r.text); } catch { /* not JSON */ }
  return new UploadError(messageFor(r.status, body), r.status);
}

/** Upload one file. Resolves with the new file id (none for upload links). */
export async function uploadChunked({ endpoint, file, meta, onProgress, signal }: ChunkOpts): Promise<{ fileId: string | null }> {
  const base = `${(await getUploadBase()) || BASE_PATH}${endpoint}`;
  const md = Object.entries({ filename: file.name, content_type: file.type || "", ...meta })
    .filter(([, v]) => v !== undefined && v !== null && v !== "")
    .map(([k, v]) => `${k} ${b64(String(v))}`).join(",");

  const created = await req("POST", base, { "Upload-Length": String(file.size), "Upload-Metadata": md }, null, undefined, signal);
  if (created.status !== 201) throw errorOf(created);
  const loc = created.headers("Location") ?? "";
  const url = `${base.replace(/\/[^/]+$/, "")}/${loc}`;
  let fileId = created.headers("Upload-File-Id");
  let offset = 0;
  // Where the server is, from a PATCH or HEAD reply.
  const take = (r: { headers: (k: string) => string | null }, fallback: number) => {
    offset = Number(r.headers("Upload-Offset") ?? fallback);
    fileId = r.headers("Upload-File-Id") ?? fileId;
  };
  let tries = 0;
  onProgress?.(0, file.size);

  while (offset < file.size) {
    const end = Math.min(offset + CHUNK_BYTES, file.size);
    try {
      const r = await req("PATCH", url, { "Upload-Offset": String(offset), "Content-Type": "application/offset+octet-stream" },
        file.slice(offset, end), (n) => onProgress?.(offset + n, file.size), signal);
      if (r.status === 204) {
        take(r, end);
        tries = 0;
        onProgress?.(offset, file.size);
        continue;
      }
      if (r.status === 409 || r.status >= 500) throw new Error("retry");
      throw errorOf(r);
    } catch (e) {
      if (e instanceof UploadError || (e as Error).name === "AbortError") throw e;
      if (++tries > 5) throw new UploadError("The connection keeps dropping. Try again later.", 0);
      await sleep(Math.min(1000 * 2 ** (tries - 1), 15000));
      // Ask where the server got to and carry on from there.
      try {
        const h = await req("HEAD", url, {}, null, undefined, signal);
        if (h.status === 200) take(h, offset);
        else if (h.status === 404) throw new UploadError("The upload was cancelled on the server. Try again.", 404);
      } catch (he) { if (he instanceof UploadError) throw he; }
    }
  }
  return { fileId };
}

/** Cancel a started upload (best effort). */
export async function cancelUpload(url: string) {
  try { await req("DELETE", url, {}, null); } catch { /* ignore */ }
}
