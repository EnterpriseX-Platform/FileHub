"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Av, Ft, Pill, Tag } from "@/components/primitives";
import { fmtAgo, fmtBytes, parseJsonArray, statusTone } from "@/lib/format";
import { canMutate } from "@/lib/roles";
import type { FileRow } from "@/lib/api";

export type FileGroup = { key: string; label: string; rows: FileRow[] };

const ownerTone = (owner: string): "rose" | "cyan" | "indigo" | "amber" | "violet" | "slate" => {
  const h = [...owner].reduce((a, c) => a + c.charCodeAt(0), 0);
  return (["rose", "cyan", "indigo", "amber", "violet", "slate"] as const)[h % 6];
};

/// Interactive files table: row selection + bulk actions + a per-row ⋯ menu.
/// Grouping and column visibility are decided server-side (from the URL) and
/// passed in as `groups` / `cols`, so this component just renders + handles
/// the client-only bits (selection state, popover menus, mutations).
export function FilesTable({ groups, cols, role }: { groups: FileGroup[]; cols: string[]; role: string | null }) {
  const router = useRouter();
  const mayMutate = canMutate(role);
  const allIds = React.useMemo(() => groups.flatMap((g) => g.rows.map((r) => r.id)), [groups]);
  const [selected, setSelected] = React.useState<Set<string>>(new Set());
  const [busy, setBusy] = React.useState(false);

  // Drop selections that are no longer present (after a refresh/filter change).
  React.useEffect(() => {
    setSelected((prev) => {
      const next = new Set([...prev].filter((id) => allIds.includes(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [allIds]);

  const show = (k: string) => cols.includes(k);
  const colCount = 2 + cols.length + 1; // checkbox + name + visible optional + ⋯
  const allChecked = allIds.length > 0 && selected.size === allIds.length;

  const toggle = (id: string) =>
    setSelected((prev) => { const n = new Set(prev); n.has(id) ? n.delete(id) : n.add(id); return n; });
  const toggleAll = () => setSelected(allChecked ? new Set() : new Set(allIds));

  const downloadIds = (ids: string[]) => {
    ids.forEach((id, i) => setTimeout(() => {
      const a = document.createElement("a");
      a.href = `/filehub/api/files/${id}/download`;
      a.download = "";
      document.body.appendChild(a); a.click(); a.remove();
    }, i * 250));
  };

  const deleteIds = async (ids: string[]) => {
    if (!ids.length || !confirm(`Move ${ids.length} file${ids.length === 1 ? "" : "s"} to Trash?`)) return;
    setBusy(true);
    try {
      await Promise.all(ids.map((id) =>
        fetch(`/filehub/api/files/${encodeURIComponent(id)}`, { method: "DELETE", credentials: "include" })));
      setSelected(new Set());
      router.refresh();
    } finally { setBusy(false); }
  };

  return (
    <>
      <table className="tbl">
        <thead>
          <tr>
            <th style={{ width: 32 }}>
              <span
                className={"cb" + (allChecked ? " on" : "")}
                role="checkbox"
                aria-checked={allChecked}
                onClick={toggleAll}
                style={{ cursor: "pointer" }}
              />
            </th>
            <th>Name</th>
            {show("status")   && <th style={{ width: 100 }}>Status</th>}
            {show("project")  && <th style={{ width: 110 }}>Project</th>}
            {show("owner")    && <th style={{ width: 140 }}>Owner</th>}
            {show("size")     && <th style={{ width: 90, textAlign: "right" }}>Size</th>}
            {show("modified") && <th style={{ width: 110 }}>Modified</th>}
            {show("tags")     && <th>Tags</th>}
            {show("versions") && <th style={{ width: 80 }}>Versions</th>}
            <th style={{ width: 36 }} />
          </tr>
        </thead>
        <tbody>
          {groups.map((g) => (
            <React.Fragment key={g.key}>
              {g.label && (
                <tr>
                  <td colSpan={colCount} style={{ background: "var(--bg-subtle)", padding: "6px 10px" }}>
                    <span className="t-xs t-subtle t-medium" style={{ textTransform: "uppercase", letterSpacing: "0.04em" }}>
                      {g.label}
                    </span>
                    <span className="t-xs t-subtle" style={{ marginLeft: 8 }}>{g.rows.length}</span>
                  </td>
                </tr>
              )}
              {g.rows.map((f) => {
                const tags = parseJsonArray(f.tags);
                const sel = selected.has(f.id);
                return (
                  <tr key={f.id} style={sel ? { background: "var(--accent-soft)" } : undefined}>
                    <td>
                      <span
                        className={"cb" + (sel ? " on" : "")}
                        role="checkbox"
                        aria-checked={sel}
                        onClick={() => toggle(f.id)}
                        style={{ cursor: "pointer" }}
                      />
                    </td>
                    <td>
                      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                        <Ft type={f.file_type} />
                        <a href={`/files/${f.id}`} className="t-medium" style={{ color: "var(--text)" }}>{f.name}</a>
                        {f.version > 1 && <Pill tone="slate" sm>v{f.version}</Pill>}
                      </div>
                    </td>
                    {show("status") && <td><Pill tone={statusTone(f.status)}><span className="dot" />{f.status}</Pill></td>}
                    {show("project") && <td>{f.project ? <Pill tone="indigo">{f.project}</Pill> : <span className="t-subtle">—</span>}</td>}
                    {show("owner") && (
                      <td>
                        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                          <Av name={f.owner} tone={ownerTone(f.owner)} />
                          <span className="t-sm">{f.owner}</span>
                        </div>
                      </td>
                    )}
                    {show("size") && (
                      <td className="t-mono t-sm t-muted" style={{ textAlign: "right" }}>
                        {f.file_type === "fold" ? "—" : fmtBytes(f.size_bytes)}
                      </td>
                    )}
                    {show("modified") && <td className="t-sm t-muted">{fmtAgo(f.modified_at)}</td>}
                    {show("tags") && (
                      <td>
                        <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
                          {tags.map((t) => <Tag key={t}>{t}</Tag>)}
                        </div>
                      </td>
                    )}
                    {show("versions") && <td className="t-mono t-sm t-muted">v{f.version}</td>}
                    <td><RowMenu file={f} mayMutate={mayMutate} onDownload={() => downloadIds([f.id])} onDelete={() => deleteIds([f.id])} /></td>
                  </tr>
                );
              })}
            </React.Fragment>
          ))}
          {allIds.length === 0 && (
            <tr><td colSpan={colCount} style={{ padding: 40, textAlign: "center", color: "var(--text-subtle)" }}>No files in this view.</td></tr>
          )}
        </tbody>
      </table>

      {selected.size > 0 && (
        <div
          style={{
            position: "sticky", bottom: 12, margin: "12px auto 0", maxWidth: 560,
            display: "flex", alignItems: "center", gap: 10, padding: "8px 12px",
            background: "var(--text)", color: "var(--bg)", borderRadius: 10, boxShadow: "var(--sh-popover)",
          }}
        >
          <span className="t-sm t-semibold">{selected.size} selected</span>
          <div style={{ flex: 1 }} />
          <button className="btn xs" style={{ background: "transparent", color: "inherit", borderColor: "currentColor" }}
            disabled={busy} onClick={() => downloadIds([...selected])}>
            <Ico.download className="icon sm" /> Download
          </button>
          {mayMutate && (
            <button className="btn xs danger" disabled={busy} onClick={() => deleteIds([...selected])}>
              <Ico.trash className="icon sm" /> {busy ? "…" : "Delete"}
            </button>
          )}
          <button className="btn xs ghost" style={{ color: "inherit" }} onClick={() => setSelected(new Set())}>Clear</button>
        </div>
      )}
    </>
  );
}

// Per-row ⋯ actions: open / download (everyone) + share / delete (editor+).
function RowMenu({ file, mayMutate, onDownload, onDelete }: {
  file: FileRow; mayMutate: boolean; onDownload: () => void; onDelete: () => void;
}) {
  const [open, setOpen] = React.useState(false);
  const ref = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);

  const Item = ({ children, onClick, danger }: { children: React.ReactNode; onClick: () => void; danger?: boolean }) => (
    <button type="button" className={"btn xs ghost" + (danger ? " danger" : "")}
      style={{ width: "100%", justifyContent: "flex-start" }}
      onClick={() => { setOpen(false); onClick(); }}>
      {children}
    </button>
  );

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button type="button" className="btn xs ghost icon row-actions" title="Actions" onClick={() => setOpen((v) => !v)}>
        <Ico.more className="icon sm" style={{ color: "var(--text-muted)" }} />
      </button>
      {open && (
        <div className="card" style={{ position: "absolute", right: 0, top: "calc(100% + 4px)", zIndex: 50, width: 160, padding: 6, boxShadow: "var(--sh-popover)", display: "flex", flexDirection: "column", gap: 2 }}>
          <a className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} href={`/files/${file.id}`}><Ico.eye className="icon sm" /> Open</a>
          <Item onClick={onDownload}><Ico.download className="icon sm" /> Download</Item>
          {mayMutate && <a className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} href={`/share?file=${encodeURIComponent(file.id)}`}><Ico.share className="icon sm" /> Share</a>}
          {mayMutate && (
            <>
              <div className="divider" style={{ margin: "4px 0" }} />
              <Item danger onClick={onDelete}><Ico.trash className="icon sm" /> Delete</Item>
            </>
          )}
        </div>
      )}
    </div>
  );
}
