"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Av, Pill } from "@/components/primitives";
import { WorkflowPanel } from "@/components/workflow-panel";
import { useAuth } from "@/lib/auth-context";
import { fmtAgo, fmtBytes } from "@/lib/format";
import { canMutate } from "@/lib/roles";

// Backend ships `Vec<CommentWithAuthor>` — display_name + avatar_tone come
// from the join with users; user_id can be null if the author was deleted.
type Comment = {
  id: string;
  file_id: string;
  user_id: string | null;
  parent_id: string | null;
  body: string;
  display_name: string;
  avatar_tone: string;
  created_at: string;
};

type Version = {
  id: string;
  file_id: string;
  version: number;
  object_key: string;
  size_bytes: number;
  etag: string | null;
  uploaded_by: string;
  note: string | null;
  created_at: string;
};

/// Right-inspector tail section: surfaces the three P1 features
/// (comments, version history, review workflow) that previously only
/// existed at the API layer.  Server-rendered inspector keeps the static
/// fields above; this component lazy-loads the dynamic parts so the page
/// stays fast.
export function FileSidecar({ fileId }: { fileId: string }) {
  return (
    <div style={{ borderTop: "1px solid var(--border)", padding: "12px 16px" }}>
      <VersionsBlock fileId={fileId} />
      <div className="divider" />
      <WorkflowPanel fileId={fileId} />
      <div className="divider" />
      <CommentsBlock fileId={fileId} />
    </div>
  );
}

// -----------------------------------------------------------------------------
// Versions
// -----------------------------------------------------------------------------
function VersionsBlock({ fileId }: { fileId: string }) {
  const router = useRouter();
  const { user } = useAuth();
  const [versions, setVersions] = React.useState<Version[] | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);

  const reload = React.useCallback(async () => {
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/versions`, {
        credentials: "include", cache: "no-store",
      });
      if (r.ok) setVersions(await r.json());
    } catch { /* show empty state */ }
  }, [fileId]);

  React.useEffect(() => { reload(); }, [reload]);

  const restore = async (v: number) => {
    setBusy(true); setErr(null);
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/versions/${v}/restore`, {
        method: "POST", credentials: "include",
      });
      if (!r.ok) {
        let detail = `HTTP ${r.status}`;
        try { detail = (await r.json()).error ?? detail; } catch { /* keep */ }
        setErr(detail);
        return;
      }
      await reload();
      router.refresh(); // header + inspector show the new current version
    } finally { setBusy(false); }
  };

  return (
    <section>
      <SectionLabel>Version history <Counter n={versions?.length ?? 0} /></SectionLabel>
      {err && <div className="t-xs" style={{ color: "var(--danger)", marginBottom: 4 }}>{err}</div>}
      {versions === null ? <Loading /> : versions.length === 0 ? (
        <div className="t-xs t-subtle">This is the current version. Each new upload is saved here so you can compare or restore earlier ones.</div>
      ) : (
        <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "flex", flexDirection: "column", gap: 6 }}>
          {versions.map((v) => (
            <li key={v.id} style={{ display: "flex", gap: 8, padding: "6px 0", borderTop: "1px solid var(--border-subtle)" }}>
              <Pill sm>v{v.version}</Pill>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="t-sm t-trunc">{v.note ?? `uploaded by ${v.uploaded_by}`}</div>
                <div className="t-xs t-subtle">{fmtBytes(v.size_bytes)} · {fmtAgo(v.created_at)}</div>
              </div>
              {canMutate(user?.role ?? null) && (
                <button
                  className="btn xs ghost"
                  disabled={busy}
                  onClick={() => restore(v.version)}
                  title={`Restore v${v.version} as the current version`}
                  aria-label={`Restore version ${v.version}`}
                >
                  <Ico.history className="icon sm" />
                </button>
              )}
              <a
                className="btn xs ghost"
                href={`/filehub/api/files/${encodeURIComponent(fileId)}/download?version=${v.version}`}
                title={`Download v${v.version}`}
                aria-label={`Download version ${v.version}`}
              >
                <Ico.download className="icon sm" />
              </a>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

// -----------------------------------------------------------------------------
// Comments
// -----------------------------------------------------------------------------
function CommentsBlock({ fileId }: { fileId: string }) {
  const { user } = useAuth();
  const [items, setItems] = React.useState<Comment[] | null>(null);
  const [draft, setDraft] = React.useState("");
  const [busy, setBusy]   = React.useState(false);
  const [err, setErr]     = React.useState<string | null>(null);

  const reload = React.useCallback(async () => {
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/comments`, {
        credentials: "include", cache: "no-store",
      });
      if (r.ok) setItems(await r.json());
    } catch { /* keep last */ }
  }, [fileId]);

  React.useEffect(() => { reload(); }, [reload]);

  const submit = async () => {
    if (!draft.trim() || busy) return;
    setBusy(true); setErr(null);
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/comments`, {
        method: "POST", credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ body: draft.trim() }),
      });
      if (!r.ok) {
        const ctype = r.headers.get("content-type") ?? "";
        let detail = `HTTP ${r.status}`;
        if (ctype.includes("application/json")) {
          try { detail += `: ${(await r.json()).error ?? ""}`; } catch { /* keep */ }
        }
        setErr(detail);
        return;
      }
      setDraft("");
      await reload();
    } finally { setBusy(false); }
  };

  return (
    <section>
      <SectionLabel>Comments <Counter n={items?.length ?? 0} /></SectionLabel>
      {items === null ? <Loading /> : items.length === 0 ? (
        <div className="t-xs t-subtle" style={{ marginBottom: 8 }}>Be the first to comment — share feedback, ask a question, or note an approval.</div>
      ) : (
        <ul style={{ listStyle: "none", padding: 0, margin: "0 0 8px", display: "flex", flexDirection: "column", gap: 8 }}>
          {items.map((c) => (
            <li key={c.id} style={{ display: "flex", gap: 8 }}>
              <Av name={c.display_name} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="t-sm">
                  <span className="t-semibold">{c.display_name}</span>{" "}
                  <span className="t-xs t-subtle" title={new Date(c.created_at).toLocaleString()}>{fmtAgo(c.created_at)}</span>
                </div>
                <div className="t-sm" style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{c.body}</div>
              </div>
            </li>
          ))}
        </ul>
      )}

      {user ? (
        <div className="field" style={{ flexDirection: "column", alignItems: "stretch", padding: "var(--sp-2)" }}>
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                submit();
              }
            }}
            rows={2}
            placeholder="Add feedback, a question, or an approval…"
            style={{ width: "100%", border: 0, background: "transparent", color: "inherit", font: "inherit", resize: "vertical" }}
          />
          {err && <div className="t-xs" style={{ color: "var(--danger)", marginTop: 4 }}>{err}</div>}
          <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 4 }}>
            <button className="btn xs primary" onClick={submit} disabled={busy || !draft.trim()}>
              {busy ? "Posting…" : "Post"}
            </button>
          </div>
        </div>
      ) : (
        <div className="t-xs t-subtle">Sign in to comment.</div>
      )}
    </section>
  );
}

// -----------------------------------------------------------------------------
// Tiny shared bits
// -----------------------------------------------------------------------------
function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="t-xs t-subtle t-medium" style={{
      letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 6,
      display: "flex", alignItems: "center", gap: 6,
    }}>
      {children}
    </div>
  );
}

function Counter({ n }: { n: number }) {
  if (n <= 0) return null;
  return <span className="t-xs t-subtle" style={{ marginLeft: "auto" }}>{n}</span>;
}

function Loading() {
  return <div className="t-xs t-subtle">Loading…</div>;
}
