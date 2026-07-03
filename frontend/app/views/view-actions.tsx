"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";

/// Per-row pin toggle + delete for saved views (PATCH/DELETE /api/views/:id —
/// creator or admin; the backend enforces, we just refresh on success).
export function ViewActions({ id, pinned }: { id: string; pinned: boolean }) {
  const router = useRouter();
  const [busy, setBusy] = React.useState(false);

  const togglePin = async () => {
    setBusy(true);
    try {
      const r = await fetch(`/filehub/api/views/${encodeURIComponent(id)}`, {
        method: "PATCH",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ pinned: !pinned }),
      });
      if (r.ok) router.refresh();
    } finally { setBusy(false); }
  };

  const remove = async () => {
    setBusy(true);
    try {
      const r = await fetch(`/filehub/api/views/${encodeURIComponent(id)}`, {
        method: "DELETE",
        credentials: "include",
      });
      if (r.ok) router.refresh();
    } finally { setBusy(false); }
  };

  return (
    <span style={{ display: "inline-flex", gap: 4 }}>
      <button
        type="button"
        className="btn xs ghost icon"
        disabled={busy}
        title={pinned ? "Unpin from sidebar + dashboard" : "Pin to sidebar + dashboard"}
        aria-label={pinned ? "Unpin view" : "Pin view"}
        onClick={togglePin}
      >
        <Ico.pin className="icon sm" style={pinned ? { color: "var(--c-amber)" } : undefined} />
      </button>
      <button
        type="button"
        className="btn xs ghost icon"
        disabled={busy}
        title="Delete view"
        aria-label="Delete view"
        onClick={remove}
      >
        <Ico.trash className="icon sm" />
      </button>
    </span>
  );
}
