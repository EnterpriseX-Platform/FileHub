"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Ft, Pill } from "@/components/primitives";
import { useI18n } from "@/lib/i18n";
import { useViewMode } from "@/lib/view-mode";

// Mirrors backend ai_api.rs::SemanticHit.
type Hit = {
  file_id: string;
  name: string;
  file_type: string;
  system_id: string;
  snippet: string;
  seq: number;
  score: number;
};

/// Semantic search — the "find by meaning" half of the AI-native layer.
/// Posts to /fh/api/search/semantic (pgvector kNN, permission-scoped) and
/// renders ranked hits with a relevance bar + matched snippet.
export function SemanticSearch() {
  const { t } = useI18n();
  const [q, setQ] = React.useState("");
  const [hits, setHits] = React.useState<Hit[] | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);
  const [ran, setRan] = React.useState("");

  const run = React.useCallback(async (term: string) => {
    const query = term.trim();
    if (!query) return;
    setLoading(true);
    setErr(null);
    setRan(query);
    try {
      const r = await fetch(`/filehub/api/search/semantic`, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ q: query, limit: 20 }),
      });
      if (!r.ok) {
        const body = await r.json().catch(() => ({}));
        throw new Error(body.error || `search failed (${r.status})`);
      }
      setHits(await r.json());
    } catch (e) {
      setErr(e instanceof Error ? e.message : "search failed");
      setHits([]);
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
      <div className="t-3xl t-semibold" style={{ marginBottom: 4 }}>{t("search.title")}</div>
      <div className="t-sm t-muted" style={{ marginBottom: "var(--sp-4)" }}>{t("search.sub")}</div>

      <form
        onSubmit={(e) => { e.preventDefault(); run(q); }}
        className="card"
        style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 14px", marginBottom: "var(--sp-4)" }}
      >
        <span style={{ color: "var(--c-violet)", display: "inline-flex" }}><Ico.sparkle /></span>
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder={t("search.placeholder")}
          autoFocus
          style={{ flex: 1, border: 0, outline: "none", background: "transparent", font: "inherit", color: "var(--text)" }}
        />
        <button type="submit" className="btn sm primary" disabled={loading || !q.trim()}>
          {loading ? t("search.searching") : t("search.button")}
        </button>
      </form>

      {loading && <ResultSkeleton />}

      {!loading && err && (
        <div className="t-sm" style={{ color: "var(--c-rose)" }}>{err}</div>
      )}

      {!loading && !err && hits !== null && (
        hits.length === 0 ? (
          <div className="t-sm t-muted">{t("search.noMatch", { q: ran })}</div>
        ) : (
          <>
            <div className="t-xs t-subtle" style={{ marginBottom: 8 }}>
              {t("search.results", { n: hits.length })}
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {hits.map((h) => <HitCard key={h.file_id} hit={h} />)}
            </div>
          </>
        )
      )}

      {!loading && hits === null && !err && (
        <div className="t-sm t-subtle">{t("search.prompt")}</div>
      )}
    </div>
  );
}

function HitCard({ hit }: { hit: Hit }) {
  const { mode } = useViewMode();
  const pct = Math.max(0, Math.min(100, Math.round(hit.score * 100)));
  return (
    <a
      href={`/filehub/${mode === "everyday" ? "f" : "files"}/${encodeURIComponent(hit.file_id)}`}
      className="card"
      style={{ display: "flex", gap: 12, padding: 14, textDecoration: "none", color: "inherit", alignItems: "flex-start" }}
    >
      <Ft type={hit.file_type} size="lg" />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span className="t-sm t-semibold t-trunc">{hit.name}</span>
          <Pill sm>{hit.system_id}</Pill>
        </div>
        <div className="t-xs t-muted" style={{ margin: "3px 0 8px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {hit.snippet}
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
          <span style={{ flex: 1, height: 5, borderRadius: 5, background: "var(--bg-strong)", overflow: "hidden" }}>
            <span style={{ display: "block", height: "100%", width: `${pct}%`, background: "var(--c-violet)", borderRadius: 5 }} />
          </span>
          <span className="t-xs" style={{ color: "var(--c-violet)", fontWeight: 500, minWidth: 32, textAlign: "right" }}>
            {hit.score.toFixed(2)}
          </span>
        </div>
      </div>
    </a>
  );
}

function ResultSkeleton() {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {[0, 1, 2].map((i) => (
        <div key={i} className="card" style={{ padding: 14, display: "flex", gap: 12 }}>
          <div className="ai-sk" style={{ width: 36, height: 36, borderRadius: 8 }} />
          <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 7 }}>
            <div className="ai-sk" style={{ height: 13, width: "40%" }} />
            <div className="ai-sk" style={{ height: 12, width: "90%" }} />
            <div className="ai-sk" style={{ height: 5, width: "100%" }} />
          </div>
        </div>
      ))}
    </div>
  );
}
