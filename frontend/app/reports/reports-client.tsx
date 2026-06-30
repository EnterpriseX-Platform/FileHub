"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { fmtAgo } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

type Item = {
  id: string;
  name: string;
  system_id: string;
  status: string;
  state: string;
  modified_at: string;
  deleted_at: string | null;
};
type Report = { active: number; inactive: number; retention: number; deleted: number; items: Item[] };

const STATE_TONE: Record<string, "emerald" | "amber" | "rose" | "slate"> = {
  active: "emerald",
  inactive: "amber",
  retention: "rose",
  deleted: "slate",
};

export function ReportsClient() {
  const { t } = useI18n();
  const [rep, setRep] = React.useState<Report | null>(null);

  React.useEffect(() => {
    (async () => {
      try {
        const r = await fetch("/filehub/api/reports/status", { credentials: "include", cache: "no-store" });
        if (r.ok) setRep(await r.json());
      } catch { /* empty state */ }
    })();
  }, []);

  const cards: [string, number, string][] = rep
    ? [
        [t("rep.active"), rep.active, "emerald"],
        [t("rep.inactive"), rep.inactive, "amber"],
        [t("rep.retention"), rep.retention, "rose"],
        [t("rep.deleted"), rep.deleted, "slate"],
      ]
    : [];

  return (
    <div className="page">
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: "var(--sp-4)", flexWrap: "wrap" }}>
        <div className="t-3xl t-semibold">{t("nav.reports")}</div>
        <div style={{ flex: 1 }} />
        <a className="btn sm" href="/filehub/api/reports/status.csv"><Ico.download className="icon sm" /> {t("rep.exportStatus")}</a>
        <a className="btn sm" href="/filehub/api/activity/export.csv"><Ico.download className="icon sm" /> {t("rep.exportAudit")}</a>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))", gap: 12, marginBottom: "var(--sp-5)" }}>
        {cards.map(([label, n, tone]) => (
          <div key={label} className="card" style={{ padding: "16px 18px" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
              <span className="dot" style={{ background: `var(--c-${tone})` }} />
              <span className="t-sm t-muted">{label}</span>
            </div>
            <div className="t-3xl t-semibold" style={{ marginTop: 4 }}>{n.toLocaleString()}</div>
          </div>
        ))}
      </div>

      <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 8 }}>
        {t("rep.docState")}
      </div>
      <div className="card" style={{ padding: 0 }}>
        <table style={{ width: "100%", borderCollapse: "collapse", fontSize: "var(--t-sm)" }}>
          <thead>
            <tr style={{ textAlign: "left", color: "var(--text-subtle)" }}>
              <th style={{ padding: "10px 14px", fontWeight: 500 }}>{t("rep.colName")}</th>
              <th style={{ padding: "10px 14px", fontWeight: 500 }}>{t("rep.colSystem")}</th>
              <th style={{ padding: "10px 14px", fontWeight: 500 }}>{t("rep.colStatus")}</th>
              <th style={{ padding: "10px 14px", fontWeight: 500 }}>{t("rep.colState")}</th>
              <th style={{ padding: "10px 14px", fontWeight: 500 }}>{t("rep.colModified")}</th>
            </tr>
          </thead>
          <tbody>
            {(rep?.items ?? []).map((it) => (
              <tr key={it.id} style={{ borderTop: "1px solid var(--border)" }}>
                <td style={{ padding: "9px 14px" }}>
                  <a href={`/filehub/files/${encodeURIComponent(it.id)}`} style={{ color: "inherit", textDecoration: "none", fontWeight: 500 }}>{it.name}</a>
                </td>
                <td style={{ padding: "9px 14px", color: "var(--text-muted)" }}>{it.system_id}</td>
                <td style={{ padding: "9px 14px", color: "var(--text-muted)" }}>{it.status}</td>
                <td style={{ padding: "9px 14px" }}>
                  <Pill tone={STATE_TONE[it.state] ?? "slate"} sm><span className="dot" />{t(`rep.${it.state}`)}</Pill>
                </td>
                <td style={{ padding: "9px 14px", color: "var(--text-muted)" }}>{fmtAgo(it.modified_at)}</td>
              </tr>
            ))}
            {rep && rep.items.length === 0 && (
              <tr><td colSpan={5} style={{ padding: 18, textAlign: "center", color: "var(--text-subtle)" }}>{t("rep.noDocs")}</td></tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
