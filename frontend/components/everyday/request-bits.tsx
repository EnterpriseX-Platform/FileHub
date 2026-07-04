"use client";

import * as React from "react";

import { Pill } from "@/components/primitives";

/// Shared building blocks for the Requests screens. Icons + colors are now
/// data-driven (each admin-authored form carries an `icon` glyph key and a
/// `color` token name), so custom forms render without any code change.

export const GLYPHS = ["expense", "it", "document", "leave", "generic"] as const;
export type GlyphKey = (typeof GLYPHS)[number];

// The design-token colors a form may use. Kept in sync with the Form Designer's
// color picker; unknown names fall back to slate.
export const FORM_COLORS = ["indigo", "emerald", "amber", "rose", "violet", "cyan", "fuchsia", "slate"] as const;
export function colorVar(color: string): string {
  return (FORM_COLORS as readonly string[]).includes(color) ? `var(--c-${color})` : "var(--c-slate)";
}

const svgProps = {
  viewBox: "0 0 24 24", fill: "none", stroke: "currentColor",
  strokeWidth: 1.7, strokeLinecap: "round" as const, strokeLinejoin: "round" as const,
};

export function KindGlyph({ icon, size = 18 }: { icon: string; size?: number }) {
  const p = { width: size, height: size, ...svgProps };
  switch (icon) {
    case "it":
      return (<svg {...p}><rect x="3" y="4.5" width="18" height="12" rx="1.5" /><path d="M8.5 20.5h7M12 16.5v4" /></svg>);
    case "document":
      return (<svg {...p}><path d="M4 20.5h16" /><path d="M14.5 5l4 4L9 18.5 4.5 20 6 15.5Z" /></svg>);
    case "leave":
      return (<svg {...p}><circle cx="12" cy="12" r="4" /><path d="M12 2.5v2.5M12 19v2.5M4.5 4.5l1.8 1.8M17.7 17.7l1.8 1.8M2.5 12H5M19 12h2.5M4.5 19.5l1.8-1.8M17.7 6.3l1.8-1.8" /></svg>);
    case "expense":
      return (<svg {...p}><rect x="2.5" y="6" width="19" height="12" rx="2" /><circle cx="12" cy="12" r="2.5" /><path d="M6 9.5v.01M18 14.5v.01" /></svg>);
    default: // generic — a form/clipboard
      return (<svg {...p}><rect x="5" y="4" width="14" height="17" rx="2" /><path d="M9 4.5V3.5h6v1" /><path d="M9 9.5h6M9 13h6M9 16.5h4" /></svg>);
  }
}

/// Tinted rounded tile with the form's glyph, in the form's color.
export function KindTile({ icon, color, size = 38 }: { icon: string; color: string; size?: number }) {
  const c = colorVar(color);
  return (
    <div
      style={{
        width: size, height: size, borderRadius: size * 0.24, flexShrink: 0,
        display: "grid", placeItems: "center", color: c,
        background: `color-mix(in srgb, ${c} 12%, transparent)`,
      }}
    >
      <KindGlyph icon={icon} size={size * 0.47} />
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
    cancelled: { tone: "slate",   key: "req.st.cancelled" },
  };
  const m = map[status] ?? map.submitted;
  return (<Pill tone={m.tone} sm><span className="dot" />{t(m.key)}</Pill>);
}

export function money(n: number | null | undefined): string {
  if (n === null || n === undefined) return "";
  return "฿" + Number(n).toLocaleString(undefined, { maximumFractionDigits: 2 });
}
