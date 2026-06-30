"use client";

import * as React from "react";

import { useAuth } from "@/lib/auth-context";

// Dynamic, per-viewer watermark (MEA TOR 5.3.5.12). Tiles the current viewer's
// identity + the document status + a timestamp diagonally across the preview,
// so any screen-grab is traceable. Rendered as a tiled SVG data-URI background
// over the preview area; pointer-events:none so it never blocks interaction.
// (This covers the *view* half; baking the same stamp into downloaded bytes is
// the server-side follow-up.)

// Map FileHub status → the Thai status labels the TOR enumerates.
const STATUS_TH: Record<string, string> = {
  draft: "เอกสารร่าง",
  active: "ใช้งาน",
  cancelled: "ยกเลิก",
  canceled: "ยกเลิก",
  published: "สำหรับเผยแพร่เพื่อเป็นข้อมูล",
  confidential: "ห้ามมิให้เผยแพร่",
};

function esc(s: string): string {
  return s.replace(/[<>&]/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;" }[c] as string));
}

export function WatermarkOverlay({ status }: { status?: string }) {
  const { user } = useAuth();
  // Recompute the timestamp on mount only (avoids re-tiling every render).
  const stamp = React.useMemo(() => new Date().toLocaleString(), []);
  if (!user) return null;

  const st = status ? STATUS_TH[status.toLowerCase()] ?? status : "";
  const line1 = `${user.email} · ${user.display_name}`;
  const line2 = `การไฟฟ้านครหลวง${st ? " · " + st : ""}`;

  const W = 340;
  const H = 200;
  const svg =
    `<svg xmlns='http://www.w3.org/2000/svg' width='${W}' height='${H}'>` +
    `<g transform='rotate(-28 ${W / 2} ${H / 2})' fill='#7c3aed' fill-opacity='0.11' ` +
    `font-family='IBM Plex Sans, sans-serif' font-weight='600'>` +
    `<text x='16' y='92' font-size='13'>${esc(line1)}</text>` +
    `<text x='16' y='112' font-size='13'>${esc(line2)}</text>` +
    `<text x='16' y='132' font-size='11' font-weight='400'>${esc(stamp)}</text>` +
    `</g></svg>`;
  const uri = `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;

  return (
    <div
      aria-hidden="true"
      style={{
        position: "absolute",
        inset: 0,
        pointerEvents: "none",
        backgroundImage: uri,
        backgroundRepeat: "repeat",
        zIndex: 5,
      }}
    />
  );
}
