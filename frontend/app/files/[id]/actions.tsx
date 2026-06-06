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

  return (
    <>
      <div style={{ display: "flex", gap: 6, marginTop: 12 }}>
        <button
          className="btn sm primary"
          style={{ flex: 1, justifyContent: "center" }}
          disabled={busy !== "" || currentStatus === "Approved"}
          onClick={() => patch("approve", { status: "Approved" })}
        >
          <Ico.check /> {busy === "approve" ? "Approving…" : currentStatus === "Approved" ? "Approved" : "Approve"}
        </button>
        <button
          className="btn sm"
          style={{ flex: 1, justifyContent: "center" }}
          disabled={busy !== "" || currentStatus === "Review"}
          onClick={() => patch("review", { status: "Review" })}
        >
          {busy === "review" ? "…" : "Request changes"}
        </button>
        <button
          className="btn sm danger icon"
          title="Delete file"
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
