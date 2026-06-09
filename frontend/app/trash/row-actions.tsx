"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { canMutate, isAdmin } from "@/lib/roles";

export function TrashRowActions({ fileId, fileName, role }: { fileId: string; fileName: string; role: string | null }) {
  const router = useRouter();
  const [busy, setBusy] = React.useState<"restore" | "purge" | "">("");
  const [error, setError] = React.useState<string>("");

  const restore = async () => {
    setBusy("restore"); setError("");
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/restore`, {
        method: "POST",
        credentials: "include",
      });
      if (r.status === 401) { window.location.href = "/login?next=/trash"; return; }
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      router.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy("");
    }
  };

  const purge = async () => {
    if (!confirm(`Permanently delete "${fileName}"? Admin role required. This cannot be undone.`)) return;
    setBusy("purge"); setError("");
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}?hard=true`, {
        method: "DELETE",
        credentials: "include",
      });
      if (r.status === 401) { window.location.href = "/login?next=/trash"; return; }
      if (r.status === 403) { setError("Admin role required to permanently delete. Contact your workspace admin."); return; }
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      router.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy("");
    }
  };

  // Mirror the backend: restore is editor+, hard-delete (purge) is admin-only.
  // Viewers see neither — the backend would 403 both — so show a read-only hint
  // instead of a button that bounces.
  const showRestore = canMutate(role);
  const showPurge   = isAdmin(role);

  return (
    <div style={{ display: "flex", gap: 6, justifyContent: "flex-end", alignItems: "center" }}>
      {showRestore && (
        <button className="btn xs" onClick={restore} disabled={busy !== ""}>
          <Ico.refresh className="icon sm" /> {busy === "restore" ? "…" : "Restore"}
        </button>
      )}
      {showPurge && (
        <button className="btn xs danger" onClick={purge} disabled={busy !== ""}>
          <Ico.trash className="icon sm" /> {busy === "purge" ? "…" : "Purge"}
        </button>
      )}
      {!showRestore && !showPurge && <span className="t-xs t-subtle">View only</span>}
      {error && <span className="t-xs" style={{ color: "var(--danger)" }}>{error}</span>}
    </div>
  );
}
