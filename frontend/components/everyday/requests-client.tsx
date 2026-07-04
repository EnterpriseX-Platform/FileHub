"use client";

import Link from "next/link";
import * as React from "react";

import { Ico } from "@/components/icons";
import { KindTile, StatusPill, money } from "@/components/everyday/request-bits";
import type { RequestListItem } from "@/lib/api";
import { fmtAgo } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

/// The Requests area: two tabs — "Waiting on you" (requests where the caller is
/// an assigned reviewer) and "My requests" (the caller's own submissions) — over
/// a prominent "New request" entry. Rows open the friendly detail view.
type Box = "inbox" | "mine" | "all";

export function RequestsClient({ inbox, mine }: { inbox: RequestListItem[]; mine: RequestListItem[] }) {
  const { t } = useI18n();
  const [box, setBox] = React.useState<Box>(inbox.length ? "inbox" : "mine");
  const [q, setQ] = React.useState("");

  const all = React.useMemo(() => {
    const seen = new Set<string>();
    return [...inbox, ...mine].filter((r) => (seen.has(r.id) ? false : (seen.add(r.id), true)));
  }, [inbox, mine]);

  const base = box === "inbox" ? inbox : box === "mine" ? mine : all;
  const needle = q.trim().toLowerCase();
  const rows = needle
    ? base.filter((r) => (r.title + " " + r.requester_name + " " + r.kind_label).toLowerCase().includes(needle))
    : base;

  const tabs: { key: Box; label: string; count: number }[] = [
    { key: "inbox", label: t("req.inbox"), count: inbox.length },
    { key: "mine", label: t("req.mine"), count: mine.length },
    { key: "all", label: t("req.all"), count: all.length },
  ];

  return (
    <div className="page">
      <div style={{ display: "flex", alignItems: "flex-start", gap: 14, flexWrap: "wrap", marginBottom: 18 }}>
        <div style={{ flex: 1, minWidth: 220 }}>
          <div className="t-3xl t-semibold">{t("req.title")}</div>
          <div className="t-sm t-muted" style={{ marginTop: 2 }}>{t("req.sub")}</div>
        </div>
        <Link href="/requests/new" className="btn primary">
          <span style={{ display: "inline-flex", color: "var(--on-accent)" }}><Ico.sparkle className="icon sm" /></span> {t("req.new")}
        </Link>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <div className="req-tabs" role="tablist">
          {tabs.map((tb) => (
            <button
              key={tb.key}
              role="tab"
              aria-selected={box === tb.key}
              className={"req-tab" + (box === tb.key ? " on" : "")}
              onClick={() => setBox(tb.key)}
            >
              {tb.label}
              {tb.count > 0 && <span className="req-tabcount">{tb.count}</span>}
            </button>
          ))}
        </div>
        <div style={{ flex: 1 }} />
        <input className="req-search" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("req.searchPh")} />
      </div>

      <div style={{ marginTop: 16 }}>
        {rows.length === 0 ? (
          <div className="card" style={{ padding: "34px 20px", textAlign: "center" }}>
            <div className="t-sm t-subtle">{box === "inbox" ? t("req.emptyInbox") : t("req.emptyMine")}</div>
          </div>
        ) : (
          <div className="card" style={{ overflow: "hidden" }}>
            {rows.map((r) => <Row key={r.id} r={r} t={t} />)}
          </div>
        )}
      </div>
    </div>
  );
}

function Row({ r, t }: { r: RequestListItem; t: (k: string, p?: Record<string, string | number>) => string }) {
  return (
    <Link href={`/requests/${encodeURIComponent(r.id)}`} className="eday-filerow">
      <KindTile icon={r.icon} color={r.color} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="t-md t-semibold t-trunc">{r.title}</div>
        <div className="t-xs t-subtle t-trunc">
          {r.kind_label}{r.amount != null ? ` · ${money(r.amount)}` : ""} · {t("req.submittedBy", { name: r.requester_name })} · {fmtAgo(r.created_at)}
        </div>
      </div>
      <StatusPill status={r.status} myTurn={r.my_turn} t={t} />
    </Link>
  );
}
