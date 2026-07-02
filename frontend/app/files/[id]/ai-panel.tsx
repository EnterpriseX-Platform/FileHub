"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill, Tag } from "@/components/primitives";
import { fmtAgo } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

// Shape of GET /fh/api/files/:id/ai (see backend ai_api.rs::FileAiResponse).
type FileAi = {
  summary: string | null;
  tags: string[];
  language: string | null;
  sensitivity: string; // none | pii | confidential
  model: string | null;
  updated_at: string;
};
type FileAiResponse = { status: string; ai: FileAi | null };

const sensTone = (s: string): "amber" | "rose" | "emerald" =>
  s === "pii" ? "amber" : s === "confidential" ? "rose" : "emerald";

/// AI intelligence panel — the per-file half of the AI-native layer. Mounts at
/// the top of the file inspector. Polls while enrichment is still in flight so
/// a freshly-uploaded file fills in without a manual refresh.
export function FileAiPanel({ fileId }: { fileId: string }) {
  const { t } = useI18n();
  const [data, setData] = React.useState<FileAiResponse | null>(null);
  const [err, setErr] = React.useState(false);

  React.useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const load = async () => {
      try {
        const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/ai`, {
          credentials: "include",
          cache: "no-store",
        });
        if (cancelled) return;
        if (!r.ok) {
          setErr(true);
          return;
        }
        const d: FileAiResponse = await r.json();
        setData(d);
        if ((d.status === "queued" || d.status === "running") && !cancelled) {
          timer = setTimeout(load, 3000); // enrichment in flight — poll
        }
      } catch {
        if (!cancelled) setErr(true);
      }
    };
    load();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [fileId]);

  const busy = data && (data.status === "queued" || data.status === "running");

  return (
    <section
      className={busy ? "ai-glow ai-pulse" : undefined}
      style={{ marginBottom: 12, borderRadius: 12, padding: busy ? "10px 12px" : 0, transition: "padding .2s var(--ease)" }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 8 }}>
        <span style={{ color: "var(--c-violet)", display: "inline-flex" }}>
          <Ico.sparkle className="icon sm" />
        </span>
        <span
          className="t-xs t-subtle t-medium"
          style={{ letterSpacing: "0.04em", textTransform: "uppercase" }}
        >
          {t("ai.intelligence")}
        </span>
        {busy && <span className="t-xs t-muted" style={{ marginLeft: "auto" }}>{t("ai.analyzing")}</span>}
      </div>

      {err ? (
        <div className="t-xs t-subtle">{t("ai.unavailable")}</div>
      ) : data === null ? (
        <Skeleton />
      ) : data.ai ? (
        <div className="eday-aicard">
          {data.ai.summary && (
            <div style={{ marginBottom: 9 }}>
              {data.ai.summary}
            </div>
          )}
          {data.ai.tags.length > 0 && (
            <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginBottom: 9 }}>
              {data.ai.tags.map((t) => (
                <Tag key={t}>{t}</Tag>
              ))}
            </div>
          )}
          <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
            <Pill tone={sensTone(data.ai.sensitivity)} sm>
              <span className="dot" />
              {data.ai.sensitivity}
            </Pill>
            {data.ai.language && <Pill sm>{data.ai.language}</Pill>}
          </div>
          {data.ai.model && (
            <div className="attr">
              {data.ai.model} · {fmtAgo(data.ai.updated_at)}
            </div>
          )}
        </div>
      ) : busy ? (
        <div className="t-xs t-subtle">{t("ai.analyzingFull")}</div>
      ) : (
        <div className="t-xs t-subtle">{t("ai.none")}</div>
      )}
    </section>
  );
}

function Skeleton() {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
      <div className="ai-sk" style={{ height: 12, width: "100%" }} />
      <div className="ai-sk" style={{ height: 12, width: "92%" }} />
      <div className="ai-sk" style={{ height: 12, width: "58%" }} />
    </div>
  );
}
