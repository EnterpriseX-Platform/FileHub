"use client";

import Link from "next/link";
import * as React from "react";

import type { View } from "@/lib/api";
import { hrefForView } from "@/lib/view-href";

const COLORS_BY_NAME: Record<string, string> = {
  rose:    "#dc2626",
  amber:   "#d97706",
  emerald: "#16a34a",
  indigo:  "#4f46e5",
  violet:  "#7c3aed",
  cyan:    "#0891b2",
  fuchsia: "#c026d3",
  slate:   "#475569",
};

/// Pinned saved-views section for the sidebar.  Previously the four entries
/// (Needs review / Expiring soon / My uploads / Q1 2026 board) were
/// hardcoded — three of them had no href so clicking did nothing.  This
/// component fetches the live `/api/views` list, picks the pinned rows, and
/// builds a `/files?...` URL from each view's first filter so the click
/// actually narrows the file table.
export function SavedViewsList() {
  const [views, setViews] = React.useState<View[]>([]);

  React.useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await fetch("/filehub/api/views", { credentials: "include", cache: "no-store" });
        if (!cancelled && r.ok) setViews(await r.json());
      } catch { /* sidebar stays empty on failure */ }
    })();
    return () => { cancelled = true; };
  }, []);

  const pinned = views.filter((v) => v.pinned).slice(0, 6);
  if (pinned.length === 0) return null;

  return (
    <>
      {pinned.map((v) => (
        <Link key={v.id} href={hrefForView(v.filters)} className="side-row">
          <span style={{
            width: 14, height: 14, borderRadius: 3,
            background: resolveColor(v.color),
            display: "inline-block",
          }} />
          <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {v.name}
          </span>
        </Link>
      ))}
    </>
  );
}

function resolveColor(c: string | null): string {
  if (!c) return "var(--text-subtle)";
  if (c.startsWith("#")) return c;
  return COLORS_BY_NAME[c] ?? c;
}
