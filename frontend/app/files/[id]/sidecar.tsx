"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Av, Pill } from "@/components/primitives";
import { useAuth } from "@/lib/auth-context";
import { fmtAgo, fmtBytes } from "@/lib/format";

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

type WorkflowStep = {
  id: string;
  workflow_id: string;
  sequence: number;
  reviewer_id: string | null;
  reviewer_name: string;
  decision: string;       // 'pending' | 'approved' | 'rejected'
  decided_at: string | null;
  note: string | null;
};

type Workflow = {
  id: string;
  file_id: string;
  state: string;          // backend serializes `state` (Draft | Review | Approved | …)
  created_at: string;
};

// list_workflow returns `Vec<(Workflow, Vec<WorkflowStep>)>` — a tuple list
// the JSON shape of which is `[[workflowObj, [steps]], …]`.
type WorkflowEntry = [Workflow, WorkflowStep[]];

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
      <WorkflowBlock fileId={fileId} />
      <div className="divider" />
      <CommentsBlock fileId={fileId} />
    </div>
  );
}

// -----------------------------------------------------------------------------
// Versions
// -----------------------------------------------------------------------------
function VersionsBlock({ fileId }: { fileId: string }) {
  const [versions, setVersions] = React.useState<Version[] | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/versions`, {
          credentials: "include", cache: "no-store",
        });
        if (!cancelled && r.ok) setVersions(await r.json());
      } catch { /* show empty state */ }
    })();
    return () => { cancelled = true; };
  }, [fileId]);

  return (
    <section>
      <SectionLabel>Version history <Counter n={versions?.length ?? 0} /></SectionLabel>
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
              <a
                className="btn xs ghost"
                href={`/filehub/api/files/${encodeURIComponent(fileId)}/download?version=${v.version}`}
                title={`Download v${v.version}`}
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
// Workflow / review
// -----------------------------------------------------------------------------
function WorkflowBlock({ fileId }: { fileId: string }) {
  const [entries, setEntries] = React.useState<WorkflowEntry[] | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/workflow`, {
          credentials: "include", cache: "no-store",
        });
        if (!cancelled && r.ok) setEntries(await r.json());
      } catch { /* keep null → loading */ }
    })();
    return () => { cancelled = true; };
  }, [fileId]);

  return (
    <section>
      <SectionLabel>Review workflow</SectionLabel>
      {entries === null ? <Loading /> : entries.length === 0 ? (
        <div className="t-xs t-subtle">No approval workflow yet — route this file through reviewers to track sign-off and decisions.</div>
      ) : entries.map(([wf, steps]) => (
        <div key={wf.id} style={{ marginBottom: 10 }}>
          <div className="t-xs t-subtle" style={{ marginBottom: 6 }}>
            {wf.state} · started {fmtAgo(wf.created_at)}
          </div>
          <ol style={{ listStyle: "none", padding: 0, margin: 0, display: "flex", flexDirection: "column", gap: 4 }}>
            {steps.map((s) => (
              <li key={s.id} style={{ display: "flex", gap: 8, alignItems: "center", padding: "4px 0", borderTop: "1px solid var(--border-subtle)" }}>
                <span className="t-xs t-mono t-subtle" style={{ width: 16 }}>{s.sequence}.</span>
                <Av name={s.reviewer_name} tone="slate" />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="t-sm t-trunc">{s.reviewer_name}</div>
                  {s.note && <div className="t-xs t-subtle t-trunc">{s.note}</div>}
                </div>
                <DecisionPill decision={s.decision} />
              </li>
            ))}
          </ol>
        </div>
      ))}
    </section>
  );
}

function DecisionPill({ decision }: { decision: string }) {
  switch (decision) {
    case "approved": return <Pill tone="emerald" sm><span className="dot" />approved</Pill>;
    case "rejected": return <Pill tone="rose"    sm><span className="dot" />rejected</Pill>;
    default:         return <Pill            sm>pending</Pill>;
  }
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
        <div className="field" style={{ flexDirection: "column", alignItems: "stretch", padding: 8 }}>
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
