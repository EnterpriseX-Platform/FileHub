"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";

import { EdaySection, FileTile, ReviewRow } from "@/components/everyday/pieces";
import { Ico } from "@/components/icons";
import { UserGreeting } from "@/components/user-greeting";
import type { FileRow } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

type Area = { id: string; name: string; tone: string };

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
  const ask = (question: string) => {
    const trimmed = question.trim();
    router.push(trimmed ? `/ask?q=${encodeURIComponent(trimmed)}` : "/ask");
  };
  return (
    <div>
      <UserGreeting />
      <div className="t-sm t-muted" style={{ marginTop: 4 }}>
        {recent.length > 0 ? t("eday.subLeftOff") : t("eday.subFirst")}
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
          <button type="submit" className="btn primary sm" style={{ borderRadius: 10 }}>
            {t("eday.ask")}
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

      {canUpload && (
        <EdaySection titleKey="eday.needsReview" href="/my?status=Review" viewAll={review.length > 0}>
          {review.length === 0 ? (
            <div className="t-sm t-subtle" style={{ padding: "6px 0" }}>{t("eday.allCaughtUp")}</div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {review.map((f) => <ReviewRow key={f.id} file={f} />)}
            </div>
          )}
        </EdaySection>
      )}

      <EdaySection titleKey="eday.recent" href="/recent" viewAll={recent.length > 0}>
        {recent.length === 0 ? (
          <div className="t-sm t-subtle" style={{ padding: "6px 0" }}>{t("eday.noFiles")}</div>
        ) : (
          <div className="eday-cards">
            {recent.map((f) => <FileTile key={f.id} file={f} />)}
          </div>
        )}
      </EdaySection>

      {areas.length > 1 && (
        <EdaySection titleKey="eday.yourAreas">
          <div className="eday-cards">
            {areas.map((a) => (
              <Link key={a.id} href={`/my?area=${encodeURIComponent(a.id)}`} className="eday-areacard">
                <span className={"pill " + a.tone + " sm"} style={{ height: 22, width: 22, padding: 0, justifyContent: "center" }}>
                  <span className="dot" />
                </span>
                <span className="t-sm t-medium t-trunc" style={{ flex: 1, minWidth: 0 }}>{a.name}</span>
                <Ico.chevron className="icon sm" style={{ color: "var(--text-subtle)" }} />
              </Link>
            ))}
          </div>
        </EdaySection>
      )}
    </div>
  );
}
