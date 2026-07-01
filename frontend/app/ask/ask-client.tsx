"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Ft, Pill } from "@/components/primitives";
import { useI18n } from "@/lib/i18n";

// Mirrors backend ai_api.rs::AskResponse / Citation.
type Citation = {
  n: number;
  file_id: string;
  name: string;
  system_id: string;
  snippet: string;
  score: number;
};
type AskResponse = { answer: string; citations: Citation[] };

/// The "Ask" experience — a grounded, cited answer over your accessible
/// content. Posts to /fh/api/ask (permission-aware RAG) and renders the answer
/// with inline citation chips that deep-link to the source file, plus the
/// ranked sources beneath.
export function AskClient() {
  const { t } = useI18n();
  const EXAMPLES = [t("ask.ex1"), t("ask.ex2")];
  const [q, setQ] = React.useState("");
  const [res, setRes] = React.useState<AskResponse | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);

  const run = React.useCallback(async (term: string) => {
    const question = term.trim();
    if (!question) return;
    setQ(question);
    setLoading(true);
    setErr(null);
    setRes(null);
    try {
      const r = await fetch(`/filehub/api/ask`, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ question, limit: 6 }),
      });
      if (!r.ok) {
        const body = await r.json().catch(() => ({}));
        throw new Error(body.error || `ask failed (${r.status})`);
      }
      setRes(await r.json());
    } catch (e) {
      setErr(e instanceof Error ? e.message : "ask failed");
    } finally {
      setLoading(false);
    }
  }, []);

  // Auto-run when arriving with ?q= (handoff from the ⌘K command palette).
  React.useEffect(() => {
    const q0 = new URLSearchParams(window.location.search).get("q");
    if (q0 && q0.trim()) { setQ(q0); run(q0); }
  }, [run]);

  return (
    <div className="page">
      <div className="t-3xl t-semibold" style={{ marginBottom: 4 }}>{t("ask.title")}</div>
      <div className="t-sm t-muted" style={{ marginBottom: "var(--sp-4)" }}>{t("ask.sub")}</div>

      <form
        onSubmit={(e) => { e.preventDefault(); run(q); }}
        className="card"
        style={{ display: "flex", alignItems: "center", gap: 10, padding: "13px 15px" }}
      >
        <span style={{ color: "var(--c-violet)", display: "inline-flex" }}><Ico.sparkle /></span>
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder={t("ask.placeholder")}
          autoFocus
          style={{ flex: 1, border: 0, outline: "none", background: "transparent", font: "inherit", fontSize: 16, color: "var(--text)" }}
        />
        <button type="submit" className="btn sm primary" disabled={loading || !q.trim()}>
          {loading ? t("ask.thinking") : t("ask.button")}
        </button>
      </form>

      {!res && !loading && !err && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: 14 }}>
          {EXAMPLES.map((ex) => (
            <button key={ex} type="button" className="btn xs ghost" onClick={() => run(ex)}>{ex}</button>
          ))}
        </div>
      )}

      {loading && <AnswerSkeleton />}

      {!loading && err && <div className="t-sm" style={{ color: "var(--c-rose)", marginTop: 16 }}>{err}</div>}

      {!loading && res && (
        <div style={{ marginTop: 18 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 10 }}>
            <span className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase" }}>{t("ask.answer")}</span>
            <Pill tone="emerald" sm><span className="dot" />{t("ask.grounded")}</Pill>
            <span className="t-xs t-subtle" style={{ marginLeft: "auto", display: "inline-flex", alignItems: "center", gap: 4 }}>
              <Ico.lock className="icon sm" /> {t("ask.scoped")}
            </span>
          </div>

          <div className="t-md" style={{ lineHeight: 1.7, whiteSpace: "pre-wrap" }}>
            {renderAnswer(res.answer, res.citations)}
          </div>

          {res.citations.length > 0 && (
            <>
              <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", margin: "20px 0 9px" }}>
                {t("ask.sources")}
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {res.citations.map((c) => <SourceCard key={c.file_id} c={c} />)}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

// Replace inline [n] tokens with citation chips that link to the source file.
function renderAnswer(answer: string, citations: Citation[]): React.ReactNode[] {
  return answer.split(/(\[\d+\])/g).map((part, i) => {
    const m = /^\[(\d+)\]$/.exec(part);
    if (m) {
      const n = Number(m[1]);
      const c = citations.find((x) => x.n === n);
      if (c) {
        return (
          <a key={i} href={`/filehub/files/${encodeURIComponent(c.file_id)}`} className="cite-chip" title={c.name}>
            {n}
          </a>
        );
      }
    }
    return <React.Fragment key={i}>{part}</React.Fragment>;
  });
}

function SourceCard({ c }: { c: Citation }) {
  const pct = Math.max(0, Math.min(100, Math.round(c.score * 100)));
  return (
    <a
      href={`/filehub/files/${encodeURIComponent(c.file_id)}`}
      className="card"
      style={{ display: "flex", gap: 12, padding: 14, textDecoration: "none", color: "inherit", alignItems: "flex-start" }}
    >
      <Ft type="text" size="lg" />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span className="cite-chip">{c.n}</span>
          <span className="t-sm t-semibold t-trunc">{c.name}</span>
          <Pill sm>{c.system_id}</Pill>
        </div>
        <div className="t-xs t-muted" style={{ margin: "3px 0 8px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {c.snippet}
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
          <span style={{ flex: 1, height: 5, borderRadius: 5, background: "var(--bg-strong)", overflow: "hidden" }}>
            <span style={{ display: "block", height: "100%", width: `${pct}%`, background: "var(--c-violet)", borderRadius: 5 }} />
          </span>
          <span className="t-xs" style={{ color: "var(--c-violet)", fontWeight: 500, minWidth: 32, textAlign: "right" }}>{c.score.toFixed(2)}</span>
        </div>
      </div>
    </a>
  );
}

function AnswerSkeleton() {
  return (
    <div style={{ marginTop: 18, display: "flex", flexDirection: "column", gap: 9 }}>
      <div className="ai-sk" style={{ height: 14, width: "30%", marginBottom: 4 }} />
      <div className="ai-sk" style={{ height: 13, width: "100%" }} />
      <div className="ai-sk" style={{ height: 13, width: "96%" }} />
      <div className="ai-sk" style={{ height: 13, width: "88%" }} />
      <div className="ai-sk" style={{ height: 13, width: "55%" }} />
    </div>
  );
}
