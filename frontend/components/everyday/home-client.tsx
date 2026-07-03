"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";

import { EdaySection, FileTile, ReviewRow } from "@/components/everyday/pieces";
import { Ico } from "@/components/icons";
import { UserGreeting } from "@/components/user-greeting";
import type { FileRow, SignQueueItem } from "@/lib/api";
import { fmtAgo } from "@/lib/format";
import { Ft } from "@/components/primitives";
import { useI18n } from "@/lib/i18n";

type Area = { id: string; name: string; tone: string; count: number };

export function HomeClient({
  recent,
  review,
  signQueue = [],
  areas,
  canUpload,
}: {
  recent: FileRow[];
  review: FileRow[];
  signQueue?: SignQueueItem[];
  areas: Area[];
  canUpload: boolean;
}) {
  const { t } = useI18n();
  const router = useRouter();
  const [q, setQ] = React.useState("");
  const areaName = React.useMemo(() => {
    const m = new Map(areas.map((a) => [a.id, a.name]));
    return (systemId: string) => m.get(systemId);
  }, [areas]);
  const ask = (question: string) => {
    const trimmed = question.trim();
    router.push(trimmed ? `/ask?q=${encodeURIComponent(trimmed)}` : "/ask");
  };
  return (
    <div>
      {/* Two-line conversational hero (matches the approved prototype). */}
      <div className="eday-hero">
        <UserGreeting />
        <div className="eday-hi2">{t("eday.hiQ")}</div>
        <div className="eday-hisub">{t("eday.hiSub2")}</div>
      </div>

      {/* AI-first hero: ask bar hands off to /ask?q= (auto-runs there). */}
      <form className="ask-hero" onSubmit={(e) => { e.preventDefault(); ask(q); }}>
        <div className="ask-hero-inner">
          <span style={{ color: "var(--c-violet)", display: "inline-flex" }}><Ico.sparkle className="icon" /></span>
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={t("eday.askPlaceholder")}
            aria-label={t("eday.askPlaceholder")}
          />
          <button type="submit" className="ask-send" aria-label={t("eday.ask")}>
            <Ico.up className="icon" />
          </button>
        </div>
      </form>
      <div className="ask-sugs">
        {[t("eday.sug1"), t("eday.sug2"), t("eday.sug3")].map((s) => (
          <button key={s} type="button" className="ask-sug" onClick={() => ask(s)}>{s}</button>
        ))}
        {canUpload && (
          <Link href="/upload" className="ask-sug" style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
            <Ico.upload className="icon sm" /> {t("nav.upload")}
          </Link>
        )}
      </div>

      {/* Proactive daily brief — honest numbers only, hidden when empty. */}
      {(review.length > 0 || recent.length > 0) && (
        <div className="eday-brief">
          <span style={{ color: "var(--text-subtle)", display: "inline-flex", marginTop: 2 }}>
            <Ico.sparkle className="icon sm" />
          </span>
          <TypedBrief
            text={
              t("eday.briefPre") +
              (review.length > 0 ? t("eday.briefWait", { n: review.length }) : "") +
              (review.length > 0 && recent.length > 0 ? " · " : "") +
              (recent.length > 0 ? t("eday.briefNew", { m: recent.length }) : "")
            }
          />
        </div>
      )}

      {/* Task list: signature turns (every role) + review queue (editors). */}
      {(canUpload || signQueue.length > 0) && (
        <EdaySection titleKey="eday.waiting" href="/my?status=Review" viewAll={review.length > 0}>
          {review.length === 0 && signQueue.length === 0 ? (
            <div className="t-sm t-subtle" style={{ padding: "6px 0" }}>{t("eday.allCaughtUp")}</div>
          ) : (
            <div className="stagger" style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {signQueue.map((s) => (
                <Link key={s.signer_id} href={`/f/${encodeURIComponent(s.file_id)}`} className="eday-filerow">
                  <Ft type="file" size="lg" />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div className="t-md t-semibold t-trunc">{s.file_name}</div>
                    <div className="t-xs t-subtle t-trunc">
                      {t("eday.waitSign")} · {fmtAgo(s.created_at)}
                    </div>
                  </div>
                  <span className="t-sm" style={{ color: "var(--accent-text)", flexShrink: 0 }}>
                    {s.my_turn ? `${t("eday.signGo")} →` : t("esign.waiting")}
                  </span>
                </Link>
              ))}
              {review.map((f) => <ReviewRow key={f.id} file={f} />)}
            </div>
          )}
        </EdaySection>
      )}

      <EdaySection titleKey="eday.pickup" href="/recent" viewAll={recent.length > 0}>
        {recent.length === 0 ? (
          <div className="t-sm t-subtle" style={{ padding: "6px 0" }}>{t("eday.noFiles")}</div>
        ) : (
          <div className="eday-cards stagger">
            {recent.map((f) => <FileTile key={f.id} file={f} area={areaName(f.system_id)} />)}
          </div>
        )}
      </EdaySection>

      {areas.length > 1 && (
        <EdaySection titleKey="eday.yourAreas">
          <div className="eday-cards stagger">
            {areas.map((a) => (
              <Link key={a.id} href={`/my?area=${encodeURIComponent(a.id)}`} className="eday-areacard">
                <span className="eday-areadot" style={{ background: `var(--c-${a.tone})` }}>
                  {a.name.charAt(0).toUpperCase()}
                </span>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span className="t-sm t-semibold t-trunc" style={{ display: "block" }}>{a.name}</span>
                  <span className="t-xs t-subtle">{t("eday.docsN", { n: a.count })}</span>
                </span>
                <Ico.chevron className="icon sm" style={{ color: "var(--text-subtle)" }} />
              </Link>
            ))}
          </div>
        </EdaySection>
      )}
    </div>
  );
}

/// Types the brief in at ~160 chars/s (prototype set piece). Instant under
/// prefers-reduced-motion; retypes when the locale switches the text.
function TypedBrief({ text }: { text: string }) {
  const instant =
    typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const [len, setLen] = React.useState(instant ? text.length : 0);
  React.useEffect(() => {
    if (instant) { setLen(text.length); return; }
    setLen(0);
    let i = 0;
    const timer = setInterval(() => {
      i = Math.min(text.length, i + 2);
      setLen(i);
      if (i >= text.length) clearInterval(timer);
    }, 12);
    return () => clearInterval(timer);
  }, [text, instant]);
  return <span>{text.slice(0, len)}</span>;
}
