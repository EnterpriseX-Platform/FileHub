"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";

import { EdaySection, FileTile, ReviewRow } from "@/components/everyday/pieces";
import { Ico } from "@/components/icons";
import { UserGreeting } from "@/components/user-greeting";
import type { FileRow } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

type Area = { id: string; name: string; tone: string; count: number };

export function HomeClient({
  recent,
  review,
  areas,
  canUpload,
}: {
  recent: FileRow[];
  review: FileRow[];
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
          <span>
            {t("eday.briefPre")}
            {review.length > 0 && t("eday.briefWait", { n: review.length })}
            {review.length > 0 && recent.length > 0 && " · "}
            {recent.length > 0 && t("eday.briefNew", { m: recent.length })}
          </span>
        </div>
      )}

      {canUpload && (
        <EdaySection titleKey="eday.waiting" href="/my?status=Review" viewAll={review.length > 0}>
          {review.length === 0 ? (
            <div className="t-sm t-subtle" style={{ padding: "6px 0" }}>{t("eday.allCaughtUp")}</div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {review.map((f) => <ReviewRow key={f.id} file={f} />)}
            </div>
          )}
        </EdaySection>
      )}

      <EdaySection titleKey="eday.pickup" href="/recent" viewAll={recent.length > 0}>
        {recent.length === 0 ? (
          <div className="t-sm t-subtle" style={{ padding: "6px 0" }}>{t("eday.noFiles")}</div>
        ) : (
          <div className="eday-cards">
            {recent.map((f) => <FileTile key={f.id} file={f} area={areaName(f.system_id)} />)}
          </div>
        )}
      </EdaySection>

      {areas.length > 1 && (
        <EdaySection titleKey="eday.yourAreas">
          <div className="eday-cards">
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
