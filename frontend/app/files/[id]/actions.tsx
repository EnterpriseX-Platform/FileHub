"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";

export function FileActions({ fileId, currentStatus }: { fileId: string; currentStatus: string }) {
  const router = useRouter();
  const [busy, setBusy] = React.useState<"approve" | "review" | "delete" | "">("");
  const [error, setError] = React.useState<string>("");

  const patch = async (which: "approve" | "review", body: Record<string, unknown>) => {
    setBusy(which);
    setError("");
    try {
      const res = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}`, {
        method: "PATCH",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (res.status === 401) { window.location.href = `/login?next=/files/${fileId}`; return; }
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      router.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy("");
    }
  };

  const remove = async () => {
    if (!confirm("Move this file to Trash? You can restore it from /trash.")) return;
    setBusy("delete");
    setError("");
    try {
      const res = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}`, {
        method: "DELETE",
        credentials: "include",
      });
      if (res.status === 401) { window.location.href = `/login?next=/files/${fileId}`; return; }
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      router.push("/files");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy("");
    }
  };

  // Status-contextual actions — a file that isn't under review gets no
  // floating Approve button. Draft/other → submit; Review → decide;
  // Approved → reopen. The backend PATCH is the same status field throughout.
  const grow = { flex: 1, justifyContent: "center" } as const;
  return (
    <>
      <div style={{ display: "flex", gap: 6, marginTop: 12 }}>
        {currentStatus === "Review" ? (
          <>
            <button
              className="btn sm primary"
              style={grow}
              aria-label="Approve file"
              disabled={busy !== ""}
              onClick={() => patch("approve", { status: "Approved" })}
            >
              <Ico.check /> {busy === "approve" ? "Approving…" : "Approve"}
            </button>
            <button
              className="btn sm"
              style={grow}
              aria-label="Request file changes"
              disabled={busy !== ""}
              onClick={() => patch("review", { status: "Draft" })}
            >
              {busy === "review" ? "Sending back…" : "Request changes"}
            </button>
          </>
        ) : currentStatus === "Approved" ? (
          <button
            className="btn sm"
            style={grow}
            aria-label="Reopen review"
            disabled={busy !== ""}
            onClick={() => patch("review", { status: "Review" })}
          >
            {busy === "review" ? "Reopening…" : "Reopen review"}
          </button>
        ) : (
          <button
            className="btn sm primary"
            style={grow}
            aria-label="Submit file for review"
            disabled={busy !== ""}
            onClick={() => patch("review", { status: "Review" })}
          >
            {busy === "review" ? "Submitting…" : "Submit for review"}
          </button>
        )}
        <button
          className="btn sm danger icon"
          title="Delete file"
          aria-label="Delete file"
          disabled={busy !== ""}
          onClick={remove}
        >
          <Ico.trash className="icon sm" />
        </button>
      </div>
      {error && <div className="t-xs" style={{ color: "var(--danger)", marginTop: 8 }}>{error}</div>}
    </>
  );
}
