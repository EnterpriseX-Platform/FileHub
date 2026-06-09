"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";

const COLORS = ["#4f46e5", "#dc2626", "#d97706", "#16a34a", "#0ea5e9", "#7c3aed"];
const COLOR_NAMES = ["Indigo", "Red", "Amber", "Green", "Sky", "Violet"];

/// Compact inline form for creating a folder. The popover anchors to the
/// "New folder" button and persists via POST /api/folders. We always feed
/// it the current system filter so the new folder lives where the user is
/// looking — that avoids a confusing "where did it go" moment.
export function NewFolderButton({
  systemId,
  systemName,
  orgId,
  parentId,
}: {
  systemId?: string;
  systemName?: string;
  orgId?: string | null;
  /// Current folder the user is browsing — the new folder is created inside it
  /// (as parent_id) so it appears where they're looking, not at the root.
  parentId?: string;
}) {
  const router = useRouter();
  const [open, setOpen]   = React.useState(false);
  const [name, setName]   = React.useState("");
  const [color, setColor] = React.useState(COLORS[0]);
  const [encrypted, setEncrypted] = React.useState(false);
  const [busy, setBusy]   = React.useState(false);
  const [error, setError] = React.useState<string>("");
  const ref = React.useRef<HTMLDivElement>(null);

  // Click-outside closes the popover. Cheap because there's only one of these
  // on the page.
  React.useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);

  const submit = async () => {
    if (!name.trim() || !systemId) return;
    setBusy(true);
    setError("");
    try {
      const res = await fetch("/filehub/api/folders", {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          name: name.trim(),
          system_id: systemId,
          org_id: orgId ?? undefined,
          parent_id: parentId ?? undefined,
          color,
          encrypted,
        }),
      });
      if (res.status === 401) { window.location.href = "/login?next=" + encodeURIComponent(window.location.pathname); return; }
      if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
      setName("");
      setOpen(false);
      router.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button
        className="btn"
        onClick={() => setOpen((v) => !v)}
        title={systemId ? `Create folder in ${systemName ?? systemId}` : "Pick a system first"}
        disabled={!systemId}
      >
        <Ico.plus /> New folder
      </button>
      {open && systemId && (
        <div
          className="card"
          style={{
            position: "absolute", right: 0, top: "calc(100% + 6px)", zIndex: 50,
            width: 320, padding: 14, boxShadow: "var(--sh-popover)",
            display: "flex", flexDirection: "column", gap: 10,
          }}
        >
          <div className="t-sm t-semibold">Create folder in {systemName ?? "—"}</div>
          <div className="field" style={{ width: "100%" }}>
            <span style={{ width: 12, height: 12, borderRadius: 3, background: color }} />
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") submit(); }}
              placeholder="Folder name"
              style={{ width: "100%" }}
            />
          </div>
          <div style={{ display: "flex", gap: 6 }}>
            {COLORS.map((c, i) => (
              <button
                key={c}
                onClick={() => setColor(c)}
                style={{
                  width: 22, height: 22, borderRadius: 6,
                  border: c === color ? "2px solid var(--text)" : "1px solid var(--border)",
                  background: c, cursor: "pointer",
                }}
                aria-label={COLOR_NAMES[i]}
              />
            ))}
          </div>
          <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", fontSize: 12 }}>
            <span className={"cb" + (encrypted ? " on" : "")} onClick={() => setEncrypted(!encrypted)} />
            Mark as encrypted (folder-level flag, files still encrypted at rest)
          </label>
          {error && <div className="t-xs" style={{ color: "var(--danger)" }}>{error}</div>}
          <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
            <button className="btn xs ghost" onClick={() => setOpen(false)} disabled={busy}>Cancel</button>
            <button className="btn xs primary" onClick={submit} disabled={busy || !name.trim()}>
              {busy ? "Creating…" : "Create"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
