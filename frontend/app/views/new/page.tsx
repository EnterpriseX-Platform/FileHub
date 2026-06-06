"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import type { System, View } from "@/lib/api";

const LAYOUTS: Array<{ key: string; label: string; Ic: React.ComponentType<{ className?: string }> }> = [
  { key: "table",    label: "Table",    Ic: Ico.table },
  { key: "board",    label: "Board",    Ic: Ico.board },
  { key: "gallery",  label: "Gallery",  Ic: Ico.gallery },
  { key: "calendar", label: "Calendar", Ic: Ico.cal },
  { key: "timeline", label: "Timeline", Ic: Ico.timeline },
];

const COLORS = ["#4f46e5", "#dc2626", "#d97706", "#16a34a", "#0ea5e9", "#7c3aed"];

export default function ViewBuilderPage() {
  const router = useRouter();
  const [systems, setSystems] = React.useState<System[]>([]);
  const [existing, setExisting] = React.useState<View[]>([]);
  const [name, setName]     = React.useState("");
  const [layout, setLayout] = React.useState("table");
  const [color, setColor]   = React.useState(COLORS[0]);
  const [pinned, setPinned] = React.useState(false);
  const [sourceSystem, setSourceSystem] = React.useState<string>("");
  // Filters carried over from the /files view the user clicked "Save as new
  // view" from, so the saved view captures what they were actually looking at.
  const [carried, setCarried] = React.useState<{ status: string; project: string; owner: string }>({ status: "", project: "", owner: "" });
  const [busy, setBusy]   = React.useState(false);
  const [error, setError] = React.useState<string>("");

  React.useEffect(() => {
    Promise.all([
      fetch("/filehub/api/systems").then((r) => r.json()).catch(() => []),
      fetch("/filehub/api/views").then((r) => r.json()).catch(() => []),
    ]).then(([s, v]: [System[], View[]]) => { setSystems(s); setExisting(v); });
  }, []);

  // Prefill from the params carried by "Save as new view" (system / status /
  // project / owner / layout) and suggest a name — so the view captures the
  // filtered table you came from instead of starting blank.
  React.useEffect(() => {
    const sp = new URLSearchParams(window.location.search);
    const sys = sp.get("system_id"); if (sys) setSourceSystem(sys);
    const lay = sp.get("layout"); if (lay && LAYOUTS.some((l) => l.key === lay)) setLayout(lay);
    const status = sp.get("status") ?? "", project = sp.get("project") ?? "", owner = sp.get("owner") ?? "";
    setCarried({ status, project, owner });
    const suggestion = status ? `${status} files` : project ? `${project} files` : owner ? `${owner}'s files` : "";
    if (suggestion) setName(suggestion);
  }, []);

  const save = async () => {
    if (!name.trim()) {
      setError("View name is required");
      return;
    }
    setBusy(true);
    setError("");
    const filters: unknown[] = [];
    if (sourceSystem)    filters.push({ field: "system_id", op: "is", value: sourceSystem });
    if (carried.status)  filters.push({ field: "status",    op: "is", value: carried.status });
    if (carried.project) filters.push({ field: "project",   op: "is", value: carried.project });
    if (carried.owner)   filters.push({ field: "owner",     op: "is", value: carried.owner });
    try {
      const res = await fetch("/filehub/api/views", {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: name.trim(), layout, color, pinned, filters }),
      });
      if (res.status === 401) { window.location.href = "/login?next=" + encodeURIComponent("/views/new"); return; }
      if (!res.ok) {
        // Backend ApiError always serialises JSON ({ "error": "…" }).  If we
        // get HTML or plain text instead we're almost certainly hitting the
        // Next.js _not-found page — surface a short, actionable message
        // rather than dumping the entire <!DOCTYPE html …> blob.
        const ctype = res.headers.get("content-type") ?? "";
        let detail = `HTTP ${res.status}`;
        if (ctype.includes("application/json")) {
          try { detail += `: ${(await res.json()).error ?? res.statusText}`; } catch { /* keep status only */ }
        } else if (res.status === 404) {
          detail += ": API route not reachable.  Reload the page to refresh cached scripts.";
        } else {
          detail += `: ${res.statusText}`;
        }
        throw new Error(detail);
      }
      router.push("/");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <div className="scr">
      <Sidebar nav="views" systems={systems} />
      <TopBar
        crumbs={["Workspace", "Views", "New"]}
        title="New view"
        actions={
          <>
            <a className="btn ghost" href="/">Cancel</a>
            <button className="btn primary" onClick={save} disabled={busy || !name.trim()}>
              {busy ? "Saving…" : "Save view"}
            </button>
          </>
        }
      />
      <div className="main" style={{ display: "grid", gridTemplateColumns: "360px 1fr", overflow: "hidden" }}>
        <div style={{ borderRight: "1px solid var(--border)", overflow: "auto", padding: "20px 20px 40px" }}>
          <Section title="View name">
            <div className="field" style={{ width: "100%", height: 36 }}>
              <span style={{ width: 14, height: 14, borderRadius: 3, background: color, flexShrink: 0 }} />
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Q1 2026 Board"
                style={{ width: "100%" }}
                autoFocus
              />
            </div>
            <div className="t-xs t-subtle" style={{ marginTop: 6 }}>Saved views show up in the dashboard and sidebar.</div>
          </Section>

          <Section title="Color">
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
              {COLORS.map((c) => (
                <button
                  key={c}
                  onClick={() => setColor(c)}
                  style={{
                    width: 24, height: 24, borderRadius: 6, border: c === color ? "2px solid var(--text)" : "1px solid var(--border)",
                    background: c, cursor: "pointer",
                  }}
                  aria-label={c}
                />
              ))}
            </div>
          </Section>

          <Section title="Layout">
            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 6 }}>
              {LAYOUTS.map(({ key, label, Ic }) => (
                <button
                  key={key}
                  onClick={() => setLayout(key)}
                  className="card"
                  style={{
                    padding: "10px 4px", textAlign: "center", cursor: "pointer",
                    background: layout === key ? "var(--accent-soft)" : undefined,
                    borderColor: layout === key ? "var(--accent-border)" : undefined,
                    color: layout === key ? "var(--accent-text)" : "var(--text-muted)",
                  }}
                >
                  <Ic className="icon" />
                  <div className="t-xs" style={{ marginTop: 4 }}>{label}</div>
                </button>
              ))}
            </div>
          </Section>

          <Section title="Source system">
            <div className="field" style={{ width: "100%" }}>
              <select
                value={sourceSystem}
                onChange={(e) => setSourceSystem(e.target.value)}
                style={{ width: "100%", border: 0, background: "transparent", color: "inherit", font: "inherit" }}
              >
                <option value="">All systems</option>
                {systems.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
              </select>
            </div>
          </Section>

          {(carried.status || carried.project || carried.owner) && (
            <Section title="Captured filters">
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                {carried.status  && <Pill tone="indigo">status = {carried.status}</Pill>}
                {carried.project && <Pill tone="indigo">project = {carried.project}</Pill>}
                {carried.owner   && <Pill tone="indigo">owner = {carried.owner}</Pill>}
              </div>
              <div className="t-xs t-subtle" style={{ marginTop: 6 }}>Carried from the view you came from — saved with this view.</div>
            </Section>
          )}

          <Section title="Pin to dashboard">
            <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
              <span className={"cb" + (pinned ? " on" : "")} onClick={() => setPinned(!pinned)} />
              <span className="t-sm">Show this view in the dashboard&apos;s Pinned views section</span>
            </label>
          </Section>

          {error && (
            <div className="card" style={{ padding: 10, borderColor: "var(--danger)", color: "var(--danger)" }}>
              {error}
            </div>
          )}
        </div>

        <div style={{ background: "var(--bg-subtle)", overflow: "auto", padding: 24 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 16 }}>
            <span className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase" }}>Preview</span>
            <Pill tone="indigo">{layout}</Pill>
          </div>

          <div className="card" style={{ padding: 32, textAlign: "center", color: "var(--text-subtle)" }}>
            <div className="t-xl t-semibold" style={{ color: "var(--text)", marginBottom: 6, display: "flex", alignItems: "center", justifyContent: "center", gap: 8 }}>
              <span style={{ width: 14, height: 14, borderRadius: 3, background: color }} />
              {name || "Untitled view"}
            </div>
            <div className="t-sm">
              {sourceSystem ? `Filtered to ${systems.find((s) => s.id === sourceSystem)?.name}` : "All systems"} ·
              {pinned ? " pinned" : " not pinned"}
            </div>
          </div>

          <div style={{ marginTop: 24 }}>
            <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 8 }}>
              Existing views
            </div>
            {existing.length === 0 ? (
              <div className="t-sm t-subtle">No views yet.</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {existing.map((v) => (
                  <div key={v.id} className="card" style={{ padding: 10, display: "flex", alignItems: "center", gap: 10 }}>
                    <span style={{ width: 12, height: 12, borderRadius: 3, background: v.color ?? "#94a3b8" }} />
                    <span className="t-sm t-medium" style={{ flex: 1 }}>{v.name}</span>
                    <Pill sm>{v.layout}</Pill>
                    {v.pinned && <Pill tone="indigo" sm>pinned</Pill>}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div style={{ marginBottom: 16 }}>
      <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 6 }}>
        {title}
      </div>
      {children}
    </div>
  );
}
