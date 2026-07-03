"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { canMutate, isAdmin } from "@/lib/roles";

export function TrashRowActions({ fileId, fileName, role }: { fileId: string; fileName: string; role: string | null }) {
  const router = useRouter();
  const [busy, setBusy] = React.useState<"restore" | "purge" | "">("");
  const [error, setError] = React.useState<string>("");
  // Two-step inline confirm instead of a native confirm() — the blocking
  // dialog froze the tab (and browsers increasingly suppress it). First
  // Purge click arms; second within 4s commits.
  const [armed, setArmed] = React.useState(false);
  React.useEffect(() => {
    if (!armed) return;
    const h = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(h);
  }, [armed]);

  // fetch() rejects with the unhelpful "Failed to fetch" when the server is
  // unreachable — and when only the BACKEND is down, the Next.js proxy turns
  // it into a bare 5xx instead. Translate both to something a person can
  // act on.
  const friendly = (e: unknown) =>
    e instanceof TypeError || (e instanceof Error && /^HTTP 5\d\d$/.test(e.message))
      ? "Couldn't reach the server — try again in a moment."
      : e instanceof Error ? e.message : String(e);

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
      setError(friendly(e));
    } finally {
      setBusy("");
    }
  };

  const purge = async () => {
    // First click arms the confirm; the button text switches to "Confirm?".
    if (!armed) { setArmed(true); return; }
    setArmed(false);
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
      setError(friendly(e));
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
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 3 }}>
      <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
        {showRestore && (
          <button className="btn xs" onClick={restore} disabled={busy !== ""}>
            <Ico.refresh className="icon sm" /> {busy === "restore" ? "…" : "Restore"}
          </button>
        )}
        {showPurge && (
          <button
            className="btn xs danger"
            onClick={purge}
            disabled={busy !== ""}
            title={armed ? `Permanently delete "${fileName}" — cannot be undone` : "Permanently delete"}
          >
            <Ico.trash className="icon sm" /> {busy === "purge" ? "…" : armed ? "Confirm?" : "Purge"}
          </button>
        )}
        {!showRestore && !showPurge && <span className="t-xs t-subtle">View only</span>}
      </div>
      {/* Own line below the buttons — squeezing into the flex row wrapped the
          message into a one-word-per-line column at the table edge. */}
      {error && (
        <span className="t-xs" style={{ color: "var(--danger)", whiteSpace: "nowrap" }}>{error}</span>
      )}
    </div>
  );
}
