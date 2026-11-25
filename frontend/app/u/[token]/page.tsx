"use client";

import { useBranding } from "@/lib/branding";

import * as React from "react";

import { UploadError, uploadChunked } from "@/lib/chunked";
import { useParams } from "next/navigation";

import { bytes, choiceList, when, BASE_PATH, type Category, type FieldDef } from "@/lib/fh";

import "../../carbon.css";

type Meta = {
  title: string; message: string | null; basket: string; folder: string | null; require_login: boolean;
  signed_in: { name: string; email: string } | null; expires_at: string; files_left: number | null; requested_by: string;
  allowed_ext: string[] | null; max_file_bytes: number; fields: FieldDef[]; categories: Category[];
};
type Q = { file: File; error?: string; progress: number; done?: boolean };

/// Public upload form behind an upload link (/u/:token). No access to the folder's
/// contents — people can only add files.
export default function UploadLinkPage() {
  const brand = useBranding();
  const { token } = useParams<{ token: string }>();
  const [m, setM] = React.useState<Meta | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [name, setName] = React.useState("");
  const [email, setEmail] = React.useState("");
  const [vals, setVals] = React.useState<Record<string, string>>({});
  const [cat, setCat] = React.useState("");
  const [queue, setQueue] = React.useState<Q[]>([]);
  const [over, setOver] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [done, setDone] = React.useState<{ uploaded: { name: string }[]; failed: { name: string; error: string }[] } | null>(null);
  const [formErr, setFormErr] = React.useState<string | null>(null);
  const input = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    fetch(`${BASE_PATH}/api/upload/${encodeURIComponent(token)}`).then(async (r) => {
      const d = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(d.error ?? (r.status === 404 ? "This upload link doesn't exist." : `This link can't be used (${r.status}).`));
      setM(d);
    }).catch((e) => setErr(e.message));
  }, [token]);

  const check = (f: File) => {
    const e = (/\.([^./]+)$/.exec(f.name)?.[1] ?? "").toLowerCase();
    if (m?.allowed_ext?.length && !m.allowed_ext.includes(e)) return `.${e || "(none)"} files aren't accepted here.`;
    if (m && f.size > m.max_file_bytes) return `Larger than the ${bytes(m.max_file_bytes)} limit.`;
    return undefined;
  };
  const add = (files: FileList | File[]) => setQueue((q) => [...q, ...Array.from(files).map((f) => ({ file: f, progress: 0, error: check(f) }))]);
  const ready = queue.filter((q) => !q.error && !q.done);

  const submit = async () => {
    setFormErr(null);
    const who = m?.signed_in?.name ?? name.trim();
    if (!who) { setFormErr("Enter your name."); return; }
    const miss = (m?.fields ?? []).filter((f) => f.required && !(vals[f.key] ?? "").trim()).map((f) => f.label);
    if (miss.length) { setFormErr(`Fill in: ${miss.join(", ")}.`); return; }
    if (!ready.length) { setFormErr("Add at least one file."); return; }
    setBusy(true);
    const fields: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(vals)) if (v.trim()) fields[k] = m?.fields.find((f) => f.key === k)?.field_type === "number" && /^-?\d+(\.\d+)?$/.test(v) ? Number(v) : v.trim();
    const meta = {
      submitter_name: m?.signed_in ? undefined : who,
      submitter_email: m?.signed_in ? undefined : email.trim() || undefined,
      category_code: cat || undefined,
      fields: JSON.stringify(fields),
    };
    const uploaded: { name: string }[] = [];
    const failed: { name: string; error: string }[] = [];
    for (const q of ready) {
      const set = (patch: Partial<typeof q>) => setQueue((all) => all.map((x) => x.file === q.file ? { ...x, ...patch } : x));
      try {
        await uploadChunked({ endpoint: `/api/upload/${encodeURIComponent(token)}/chunks`, file: q.file, meta,
          onProgress: (n, t) => set({ progress: t ? Math.round((n / t) * 100) : 100 }) });
        set({ progress: 100, done: true });
        uploaded.push({ name: q.file.name });
      } catch (e) {
        const st = (e as UploadError).status;
        const msg = st === 401 ? "Sign in to your organization account first, then open this link again." : (e as Error).message;
        // Problems with the form itself stop everything; a problem with one file doesn't.
        if (st === 401 || /name|email|required|Fill in|expired|turned off|accepts/i.test(msg) && failed.length === 0 && uploaded.length === 0) { setBusy(false); setFormErr(msg); return; }
        set({ error: msg });
        failed.push({ name: q.file.name, error: msg });
      }
    }
    setBusy(false);
    setDone({ uploaded, failed });
  };

  return (
    <div className="cx-app" style={{ display: "block" }}>
      <div className="cx-nav-head">{brand.brand_name}</div>
      <div className="cx-page" style={{ maxWidth: 720, margin: "0 auto", paddingTop: 24 }}>
        {err && <div className="cx-note cx-note-error" role="alert">{err}</div>}
        {!err && !m && <div style={{ background: "var(--cx-layer)", padding: 16 }}><span className="cx-skel" style={{ width: "50%" }} /></div>}
        {m && done && (
          <div style={{ background: "var(--cx-layer)", padding: 16, display: "flex", flexDirection: "column", gap: 10 }}>
            <h1 className="cx-title">Files sent</h1>
            <div>{done.uploaded.length} file{done.uploaded.length === 1 ? "" : "s"} sent to {m.requested_by}.</div>
            <ul className="cx-list">{done.uploaded.map((u, i) => <li key={i}>{u.name}</li>)}</ul>
            {done.failed.length > 0 && <div className="cx-note cx-note-error">{done.failed.map((f) => `${f.name}: ${f.error}`).join(" ")}</div>}
            <div><button className="cx-btn cx-btn-tertiary" onClick={() => { setDone(null); setQueue([]); }}>Send more files</button></div>
          </div>
        )}
        {m && !done && (
          <div className="cx-on-layer" style={{ background: "var(--cx-layer)", padding: 16, display: "flex", flexDirection: "column", gap: 14 }}>
            <div className="cx-sub">{m.requested_by} · {m.basket}{m.folder ? ` › ${m.folder}` : ""}</div>
            <h1 className="cx-title">{m.title}</h1>
            {m.message && <div style={{ whiteSpace: "pre-wrap" }}>{m.message}</div>}
            <div className="cx-helper cx-num">Open until {when(m.expires_at)}{m.files_left != null ? ` · ${m.files_left} more file${m.files_left === 1 ? "" : "s"} accepted` : ""}</div>
            {m.require_login && !m.signed_in ? (
              <div className="cx-note cx-note-info">Sign in to your organization account first, then open this link again.
                <a className="cx-btn cx-btn-ghost" href={`${BASE_PATH}/login?next=${encodeURIComponent(`/u/${token}`)}`}>Sign in</a></div>
            ) : (
              <>
                {m.signed_in ? <div>Sending as <b style={{ fontWeight: 600 }}>{m.signed_in.name}</b> ({m.signed_in.email})</div> : (
                  <div className="cx-form-grid">
                    <Field label="Your name" required value={name} onChange={setName} />
                    <Field label="Email" type="email" value={email} onChange={setEmail} />
                  </div>
                )}
                {(m.fields.length > 0 || m.categories.length > 0) && (
                  <div className="cx-form-grid">
                    {m.categories.length > 0 && (
                      <Select label="Category" value={cat} onChange={setCat} options={[{ value: "", label: "No category" }, ...m.categories.map((c) => ({ value: c.code, label: c.name }))]} />
                    )}
                    {m.fields.map((f) => f.field_type === "choice"
                      ? <Select key={f.key} label={f.label} required={f.required} value={vals[f.key] ?? ""} onChange={(v) => setVals((x) => ({ ...x, [f.key]: v }))}
                          options={[{ value: "", label: "—" }, ...choiceList(f)]} />
                      : <Field key={f.key} label={f.label} required={f.required} type={f.field_type === "date" ? "date" : "text"} value={vals[f.key] ?? ""} onChange={(v) => setVals((x) => ({ ...x, [f.key]: v }))} />)}
                  </div>
                )}
                <div className="cx-drop" data-over={over} role="button" tabIndex={0}
                  onClick={() => input.current?.click()}
                  onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); input.current?.click(); } }}
                  onDragOver={(e) => { e.preventDefault(); setOver(true); }} onDragLeave={() => setOver(false)}
                  onDrop={(e) => { e.preventDefault(); setOver(false); add(e.dataTransfer.files); }}>
                  Drag files here or <b>browse</b>
                  <div className="cx-helper" style={{ marginTop: 4 }}>{m.allowed_ext?.length ? m.allowed_ext.map((x) => `.${x}`).join(" ") + " · " : ""}up to {bytes(m.max_file_bytes)} each</div>
                  <input ref={input} type="file" multiple hidden onChange={(e) => { if (e.target.files) add(e.target.files); e.target.value = ""; }} />
                </div>
                {queue.length > 0 && (
                  <ul className="cx-list" style={{ background: "var(--cx-layer-2)", padding: "0 12px" }}>
                    {queue.map((q, i) => (
                      <li key={i}>
                        <span className="cx-grow">{q.file.name}</span>
                        <span className="cx-sub cx-num" style={{ width: 64, textAlign: "right" }}>{bytes(q.file.size)}</span>
                        <span style={{ width: 160 }}>{q.error ? <span className="cx-error">{q.error}</span>
                          : busy ? <span className="cx-progress" style={{ display: "block" }}><i style={{ width: `${q.progress}%` }} /></span> : null}</span>
                        {!busy && <button className="cx-close" style={{ width: 24, height: 24 }} aria-label={`Remove ${q.file.name}`} onClick={() => setQueue((x) => x.filter((_, j) => j !== i))}>×</button>}
                      </li>
                    ))}
                  </ul>
                )}
                {formErr && <div className="cx-note cx-note-error" role="alert">{formErr}</div>}
                <div><button className="cx-btn cx-btn-primary" disabled={busy || ready.length === 0} onClick={submit}>
                  {busy ? "Sending…" : `Send ${ready.length} file${ready.length === 1 ? "" : "s"}`}</button></div>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function Field({ label, required, value, onChange, type = "text" }: { label: string; required?: boolean; value: string; onChange: (v: string) => void; type?: string }) {
  const id = React.useId();
  return (
    <div className="cx-field">
      <label className="cx-label" htmlFor={id}>{label}{required && <span className="cx-req">*</span>}</label>
      <input id={id} className="cx-input" type={type} value={value} onChange={(e) => onChange(e.target.value)} />
    </div>
  );
}

/// Minimal system dropdown for the public page (no native <select>).
function Select({ label, required, value, onChange, options }: { label: string; required?: boolean; value: string; onChange: (v: string) => void; options: { value: string; label: string }[] }) {
  const [open, setOpen] = React.useState(false);
  const id = React.useId();
  const cur = options.find((o) => o.value === value);
  return (
    <div className="cx-field cx-dd">
      <label className="cx-label" htmlFor={id}>{label}{required && <span className="cx-req">*</span>}</label>
      <button id={id} type="button" className="cx-dd-trigger" aria-expanded={open} onClick={() => setOpen((o) => !o)} onBlur={() => setTimeout(() => setOpen(false), 150)}>
        <span className={cur && cur.value ? "cx-dd-val" : "cx-dd-ph"}>{cur?.label ?? "Choose"}</span>
      </button>
      {open && (
        <div className="cx-dd-menu" role="listbox">
          {options.map((o) => <button key={o.value} type="button" role="option" aria-selected={o.value === value} className="cx-dd-opt" onMouseDown={() => { onChange(o.value); setOpen(false); }}>{o.label}</button>)}
        </div>
      )}
    </div>
  );
}
