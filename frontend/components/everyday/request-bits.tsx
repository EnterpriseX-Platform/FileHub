"use client";

import * as React from "react";

import { Pill } from "@/components/primitives";
import type { RequestKind } from "@/lib/api";

/// Shared building blocks for the Requests screens — kind icons (stroke SVGs
/// that match the app's icon style, no emoji), a tinted icon tile, a status
/// pill, and money formatting. Kept in one place so list / new / detail stay
/// visually consistent.

export const KIND_META: Record<RequestKind, { color: string; labelEn: string; labelTh: string }> = {
  expense:  { color: "var(--c-emerald)", labelEn: "Expense",           labelTh: "เบิกจ่าย" },
  it:       { color: "var(--c-indigo)",  labelEn: "IT / Service",      labelTh: "ไอที" },
  document: { color: "var(--c-violet)",  labelEn: "Document approval", labelTh: "อนุมัติเอกสาร" },
  leave:    { color: "var(--c-amber)",   labelEn: "Leave",             labelTh: "การลา" },
};

const svgProps = {
  viewBox: "0 0 24 24", fill: "none", stroke: "currentColor",
  strokeWidth: 1.7, strokeLinecap: "round" as const, strokeLinejoin: "round" as const,
};

export function KindGlyph({ kind, size = 18 }: { kind: RequestKind; size?: number }) {
  const p = { width: size, height: size, ...svgProps };
  switch (kind) {
    case "it":
      return (<svg {...p}><rect x="3" y="4.5" width="18" height="12" rx="1.5" /><path d="M8.5 20.5h7M12 16.5v4" /></svg>);
    case "document":
      return (<svg {...p}><path d="M4 20.5h16" /><path d="M14.5 5l4 4L9 18.5 4.5 20 6 15.5Z" /></svg>);
    case "leave":
      return (<svg {...p}><circle cx="12" cy="12" r="4" /><path d="M12 2.5v2.5M12 19v2.5M4.5 4.5l1.8 1.8M17.7 17.7l1.8 1.8M2.5 12H5M19 12h2.5M4.5 19.5l1.8-1.8M17.7 6.3l1.8-1.8" /></svg>);
    default: // expense
      return (<svg {...p}><rect x="2.5" y="6" width="19" height="12" rx="2" /><circle cx="12" cy="12" r="2.5" /><path d="M6 9.5v.01M18 14.5v.01" /></svg>);
  }
}

/// Tinted rounded tile with the kind glyph, matching the prototype's ficon.
export function KindTile({ kind, size = 38 }: { kind: RequestKind; size?: number }) {
  const color = KIND_META[kind].color;
  return (
    <div
      style={{
        width: size, height: size, borderRadius: size * 0.24, flexShrink: 0,
        display: "grid", placeItems: "center", color,
        background: `color-mix(in srgb, ${color} 12%, transparent)`,
      }}
    >
      <KindGlyph kind={kind} size={size * 0.47} />
    </div>
  );
}

type Tone = "emerald" | "indigo" | "amber" | "rose" | "slate" | "violet";

export function StatusPill({ status, myTurn, t }: {
  status: string;
  myTurn?: boolean;
  t: (k: string) => string;
}) {
  if (myTurn) {
    return (<Pill tone="amber" sm><span className="dot" />{t("req.yourturn")}</Pill>);
  }
  const map: Record<string, { tone: Tone; key: string }> = {
    approved:  { tone: "emerald", key: "req.st.approved" },
    rejected:  { tone: "rose",    key: "req.st.rejected" },
    in_review: { tone: "indigo",  key: "req.st.in_review" },
    submitted: { tone: "slate",   key: "req.st.submitted" },
  };
  const m = map[status] ?? map.submitted;
  return (<Pill tone={m.tone} sm><span className="dot" />{t(m.key)}</Pill>);
}

export function money(n: number | null | undefined): string {
  if (n === null || n === undefined) return "";
  return "฿" + Number(n).toLocaleString(undefined, { maximumFractionDigits: 2 });
}
