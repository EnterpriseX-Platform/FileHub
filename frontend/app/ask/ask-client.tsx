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
type Turn = { id: number; q: string; res: AskResponse | null; err: string | null };

/// The "Ask" experience — a conversation thread over your accessible content.
/// Each question posts to /fh/api/ask (permission-aware RAG); the newest
/// answer types itself in with a pulsing caret, with inline citation chips
/// that deep-link to the source file and ranked sources beneath. The input
/// stays pinned at the bottom for follow-ups.
export function AskClient() {
  const { t } = useI18n();
  const EXAMPLES = [t("ask.ex1"), t("ask.ex2")];
  const [q, setQ] = React.useState("");
  const [turns, setTurns] = React.useState<Turn[]>([]);
  const nextId = React.useRef(1);
  const busy = turns.some((tr) => tr.res === null && tr.err === null);
  const endRef = React.useRef<HTMLDivElement>(null);

  const run = React.useCallback(async (term: string) => {
    const question = term.trim();
    if (!question) return;
    const id = nextId.current++;
    setQ("");
    setTurns((s) => [...s, { id, q: question, res: null, err: null }]);
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
      const res: AskResponse = await r.json();
      setTurns((s) => s.map((tr) => (tr.id === id ? { ...tr, res } : tr)));
    } catch (e) {
      const err = e instanceof Error ? e.message : "ask failed";
      setTurns((s) => s.map((tr) => (tr.id === id ? { ...tr, err } : tr)));
    }
  }, []);

  // Auto-run when arriving with ?q= (handoff from ⌘K / the home ask bar).
  React.useEffect(() => {
    const q0 = new URLSearchParams(window.location.search).get("q");
    if (q0 && q0.trim()) run(q0);
  }, [run]);

  React.useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [turns]);

  return (
    <div className="page" style={{ display: "flex", flexDirection: "column", minHeight: "100%" }}>
      <div className="t-3xl t-semibold" style={{ marginBottom: 4 }}>{t("ask.title")}</div>
      <div className="t-sm t-muted" style={{ marginBottom: "var(--sp-4)" }}>{t("ask.sub")}</div>

      {turns.length === 0 && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginBottom: 14 }}>
          {EXAMPLES.map((ex) => (
            <button key={ex} type="button" className="ask-sug" onClick={() => run(ex)}>{ex}</button>
          ))}
        </div>
      )}

      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 20 }}>
        {turns.map((turn, i) => (
          <TurnBlock key={turn.id} turn={turn} latest={i === turns.length - 1} t={t} />
        ))}
        <div ref={endRef} />
      </div>

      <form
        onSubmit={(e) => { e.preventDefault(); run(q); }}
        className="ask-hero"
        style={{ position: "sticky", bottom: 16, marginTop: 20, maxWidth: "none" }}
      >
        <div className="ask-hero-inner">
          <span style={{ color: "var(--c-violet)", display: "inline-flex" }}><Ico.sparkle /></span>
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={turns.length ? t("ask.followUp") : t("ask.placeholder")}
            autoFocus
          />
          <button type="submit" className="ask-send" disabled={busy || !q.trim()}
            aria-label={busy ? t("ask.thinking") : t("ask.button")}
            style={busy || !q.trim() ? { opacity: 0.55, cursor: "not-allowed" } : undefined}>
            <Ico.up className="icon" />
          </button>
        </div>
      </form>
    </div>
  );
}

function TurnBlock({ turn, latest, t }: {
  turn: Turn;
  latest: boolean;
  t: (k: string, params?: Record<string, string | number>) => string;
}) {
  return (
    <div>
      <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 12 }}>
        <div style={{
          maxWidth: "78%", padding: "9px 14px", borderRadius: "14px 14px 4px 14px",
          background: "var(--accent-soft)", color: "var(--accent-text)",
          fontSize: "var(--t-md)", lineHeight: 1.55,
        }}>
          {turn.q}
        </div>
      </div>

      {turn.err ? (
        <div className="t-sm" style={{ color: "var(--c-rose)" }}>{turn.err}</div>
      ) : turn.res === null ? (
        <AnswerSkeleton />
      ) : (
        <div>
          <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 8 }}>
            <span className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase" }}>{t("ask.answer")}</span>
            <Pill tone="emerald" sm><span className="dot" />{t("ask.grounded")}</Pill>
            <span className="t-xs t-subtle" style={{ marginLeft: "auto", display: "inline-flex", alignItems: "center", gap: 4 }}>
              <Ico.lock className="icon sm" /> {t("ask.scoped")}
            </span>
          </div>
          <TypedAnswer answer={turn.res.answer} citations={turn.res.citations} animate={latest} />
          {turn.res.citations.length > 0 && (
            <>
              <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", margin: "16px 0 8px" }}>
                {t("ask.sources")}
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {turn.res.citations.map((c) => <SourceCard key={c.file_id} c={c} />)}
              </div>
              {/* Agentic follow-up — a REAL action only (opens the top source). */}
              <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginTop: 12 }}>
                <a className="ask-sug" href={`/filehub/f/${encodeURIComponent(turn.res.citations[0].file_id)}`}>
                  {t("ask.openSrc", { name: shortName(turn.res.citations[0].name) })} →
                </a>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/// Streaming-feel reveal: the answer types in at ~180 chars/s with a pulsing
/// caret. The API call itself is one-shot — this is presentation only, so the
/// text is complete (and copy-able) the moment the animation finishes. Only
/// the latest turn animates; older turns render instantly, as does anyone
/// with prefers-reduced-motion.
function TypedAnswer({ answer, citations, animate }: { answer: string; citations: Citation[]; animate: boolean }) {
  const instant =
    !animate ||
    (typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [len, setLen] = React.useState(instant ? answer.length : 0);

  React.useEffect(() => {
    if (instant) { setLen(answer.length); return; }
    setLen(0);
    let i = 0;
    const timer = setInterval(() => {
      i = Math.min(answer.length, i + 3);
      setLen(i);
      if (i >= answer.length) clearInterval(timer);
    }, 16);
    return () => clearInterval(timer);
  }, [answer, instant]);

  const done = len >= answer.length;
  return (
    <div className="t-md" style={{ lineHeight: 1.7, whiteSpace: "pre-wrap" }}>
      {renderAnswer(answer.slice(0, len), citations)}
      {!done && <span className="ai-caret" aria-hidden />}
    </div>
  );
}

// Keep follow-up chips compact: trim long file names to their stem.
function shortName(name: string): string {
  const stem = name.replace(/\.[a-z0-9]+$/i, "");
  return stem.length > 28 ? stem.slice(0, 27) + "…" : stem;
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
    <div style={{ display: "flex", flexDirection: "column", gap: 9 }}>
      <div className="ai-sk" style={{ height: 14, width: "30%", marginBottom: 4 }} />
      <div className="ai-sk" style={{ height: 13, width: "100%" }} />
      <div className="ai-sk" style={{ height: 13, width: "96%" }} />
      <div className="ai-sk" style={{ height: 13, width: "55%" }} />
    </div>
  );
}
