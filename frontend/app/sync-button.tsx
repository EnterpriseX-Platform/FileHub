"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";

/// Sync triggers a Next.js router refresh — that re-runs the dashboard's
/// server fetches without losing the URL, so the cards/activity/views
/// re-render with fresh data. We also track when the last sync happened so
/// the button copy reflects how stale the view is.
export function SyncButton() {
  const router = useRouter();
  const [syncedAt, setSyncedAt] = React.useState<number>(Date.now());
  const [busy, setBusy] = React.useState(false);

  // Tick once per minute so the label updates without a router round-trip.
  const [, force] = React.useReducer((n: number) => n + 1, 0);
  React.useEffect(() => {
    const t = setInterval(force, 60_000);
    return () => clearInterval(t);
  }, []);

  const onClick = async () => {
    setBusy(true);
    try {
      router.refresh();
      setSyncedAt(Date.now());
    } finally {
      // refresh() returns void; we don't actually know when SSR is done, but
      // a short timeout is enough to feel responsive without leaving the
      // button stuck in the "syncing" state.
      setTimeout(() => setBusy(false), 400);
    }
  };

  const ago = (() => {
    const secs = Math.floor((Date.now() - syncedAt) / 1000);
    if (secs < 5)   return "just synced";
    if (secs < 60)  return `synced ${secs}s ago`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `synced ${mins}m ago`;
    const hours = Math.floor(mins / 60);
    return `synced ${hours}h ago`;
  })();

  return (
    <button className="btn ghost" onClick={onClick} disabled={busy} title={ago}>
      <Ico.refresh /> {busy ? "Syncing…" : ago}
    </button>
  );
}
