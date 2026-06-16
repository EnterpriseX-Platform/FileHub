"use client";

import { useRouter } from "next/navigation";
import * as React from "react";
import { createPortal } from "react-dom";

import { Ico } from "@/components/icons";
import { Av, Ft, Pill, Tag } from "@/components/primitives";
import { fmtAgo, fmtBytes, parseJsonArray, statusTone } from "@/lib/format";
import { canMutate } from "@/lib/roles";
import type { FileRow, Folder } from "@/lib/api";

export type FileGroup = { key: string; label: string; rows: FileRow[] };

const ownerTone = (owner: string): "rose" | "cyan" | "indigo" | "amber" | "violet" | "slate" => {
  const h = [...owner].reduce((a, c) => a + c.charCodeAt(0), 0);
  return (["rose", "cyan", "indigo", "amber", "violet", "slate"] as const)[h % 6];
};

// a11y: let the keyboard toggle a role="checkbox" span (Enter/Space), mirroring
// its onClick handler.
const cbKeyDown = (toggle: () => void) => (e: React.KeyboardEvent) => {
  if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(); }
};

/// Interactive files table: row selection + bulk actions + a per-row ⋯ menu.
/// Grouping and column visibility are decided server-side (from the URL) and
/// passed in as `groups` / `cols`, so this component just renders + handles
/// the client-only bits (selection state, popover menus, mutations).
export function FilesTable({ groups, cols, role, folders = [] }: { groups: FileGroup[]; cols: string[]; role: string | null; folders?: Folder[] }) {
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

  // PATCH a JSON body onto one file. `folder_id: null` moves to the system root.
  const patchFile = (id: string, body: Record<string, unknown>) =>
    fetch(`/filehub/api/files/${encodeURIComponent(id)}`, {
      method: "PATCH",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

  // #10 — bulk move the selected files into a folder (or to the root: null).
  const moveIds = async (ids: string[], folderId: string | null) => {
    if (!ids.length) return;
    setBusy(true);
    try {
      await Promise.all(ids.map((id) => patchFile(id, { folder_id: folderId })));
      setSelected(new Set());
      router.refresh();
    } finally { setBusy(false); }
  };

  // #10 — inline rename a single file via the ⋯ menu.
  const renameFile = async (file: FileRow) => {
    const next = prompt("Rename file", file.name)?.trim();
    if (!next || next === file.name) return;
    setBusy(true);
    try {
      await patchFile(file.id, { name: next });
      router.refresh();
    } finally { setBusy(false); }
  };

  // Share: a single file navigates to /share?file=ID. With several selected we
  // share the first (the bulk-bar button label makes that explicit).
  const shareIds = (ids: string[]) => {
    if (!ids.length) return;
    router.push(`/share?file=${encodeURIComponent(ids[0])}`);
  };

  return (
    <>
      {/* ≤620px the table is unusable (one legible column), so a parallel
          card list renders instead — same selection Set, same RowMenu, same
          bulk bar. tokens.css .only-desktop/.only-mobile do the swap. */}
      <div className="only-mobile">
        {groups.map((g) => (
          <React.Fragment key={g.key}>
            {g.label && (
              <div className="t-xs t-subtle t-medium" style={{ textTransform: "uppercase", letterSpacing: "0.04em", padding: "var(--sp-2) var(--sp-1) var(--sp-1)" }}>
                {g.label} <span style={{ marginLeft: 6 }}>{g.rows.length}</span>
              </div>
            )}
            {g.rows.map((f) => {
              const sel = selected.has(f.id);
              return (
                <div
                  key={f.id}
                  style={{
                    display: "flex", alignItems: "center", gap: 4,
                    padding: "4px 0", borderBottom: "1px solid var(--border)",
                    background: sel ? "var(--accent-soft)" : undefined,
                  }}
                >
                  {/* padded wrapper = ≥40px touch target without growing the visual box */}
                  <span
                    role="checkbox"
                    aria-checked={sel}
                    aria-label={`Select ${f.name}`}
                    tabIndex={0}
                    onClick={() => toggle(f.id)}
                    onKeyDown={cbKeyDown(() => toggle(f.id))}
                    style={{ padding: "var(--sp-4)", display: "inline-flex", cursor: "pointer" }}
                  >
                    <span className={"cb" + (sel ? " on" : "")} />
                  </span>
                  <a href={`/files/${f.id}`} style={{ flex: 1, minWidth: 0, display: "flex", alignItems: "center", gap: 10, padding: "8px 0", color: "var(--text)" }}>
                    <Ft type={f.file_type} size="lg" />
                    <span style={{ flex: 1, minWidth: 0 }}>
                      <span className="t-medium t-trunc" style={{ display: "block" }}>{f.name}</span>
                      <span className="t-xs t-muted t-trunc" style={{ display: "block", marginTop: 2 }}>
                        {f.file_type === "fold" ? "" : `${fmtBytes(f.size_bytes)} · `}{fmtAgo(f.modified_at)} · {f.owner}
                      </span>
                    </span>
                    <Pill tone={statusTone(f.status)} sm><span className="dot" />{f.status}</Pill>
                  </a>
                  <RowMenu file={f} mayMutate={mayMutate} onDownload={() => downloadIds([f.id])} onDelete={() => deleteIds([f.id])} onRename={() => renameFile(f)} />
                </div>
              );
            })}
          </React.Fragment>
        ))}
        {allIds.length === 0 && (
          <div style={{ padding: 40, textAlign: "center", color: "var(--text-subtle)" }}>No files in this view.</div>
        )}
      </div>

      {/* overflow-x lets the fixed-width columns scroll at narrow desktop
          widths (861–1100px) instead of clipping the right-hand columns. */}
      <div className="only-desktop" style={{ overflowX: "auto" }}>
      <table className="tbl">
        <thead>
          <tr>
            <th style={{ width: 32 }}>
              <span
                className={"cb" + (allChecked ? " on" : "")}
                role="checkbox"
                aria-checked={allChecked}
                aria-label="Select all files"
                tabIndex={0}
                onClick={toggleAll}
                onKeyDown={cbKeyDown(toggleAll)}
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
                  <tr key={f.id} className={sel ? "sel" : undefined}>
                    <td>
                      <span
                        className={"cb" + (sel ? " on" : "")}
                        role="checkbox"
                        aria-checked={sel}
                        aria-label={`Select ${f.name}`}
                        tabIndex={0}
                        onClick={() => toggle(f.id)}
                        onKeyDown={cbKeyDown(() => toggle(f.id))}
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
                    <td><RowMenu file={f} mayMutate={mayMutate} onDownload={() => downloadIds([f.id])} onDelete={() => deleteIds([f.id])} onRename={() => renameFile(f)} /></td>
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
      </div>

      {/* Reserve space so the fixed bulk bar never covers the last row/card. */}
      {selected.size > 0 && <div aria-hidden style={{ height: 72 }} />}

      {selected.size > 0 && (
        <div
          style={{
            // Fixed (not sticky) so it floats at the viewport bottom regardless
            // of the .table-scroll / .main overflow wrappers — sticky broke once
            // the table got wrapped for horizontal scrolling, leaving the bar
            // stranded at the bottom of the content where selecting rows from the
            // top showed nothing.
            position: "fixed", left: "50%", transform: "translateX(-50%)", bottom: 16, zIndex: 50,
            // min() keeps the bar inside narrow viewports; wrap lets the
            // actions flow to a second line instead of clipping.
            maxWidth: "min(560px, calc(100vw - 16px))", width: "max-content",
            display: "flex", alignItems: "center", flexWrap: "wrap", gap: 10, padding: "8px 12px",
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
            <button className="btn xs" style={{ background: "transparent", color: "inherit", borderColor: "currentColor" }}
              disabled={busy} onClick={() => shareIds([...selected])}
              title={selected.size > 1 ? "Shares the first selected file" : "Share this file"}>
              <Ico.share className="icon sm" /> Share{selected.size > 1 ? " first" : ""}
            </button>
          )}
          {mayMutate && (
            <MoveMenu folders={folders} busy={busy} onMove={(fid) => moveIds([...selected], fid)} />
          )}
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

// Per-row ⋯ actions: open / download (everyone) + share / rename / delete (editor+).
function RowMenu({ file, mayMutate, onDownload, onDelete, onRename }: {
  file: FileRow; mayMutate: boolean; onDownload: () => void; onDelete: () => void; onRename: () => void;
}) {
  const [open, setOpen] = React.useState(false);
  const [pos, setPos] = React.useState<{ top: number; right: number } | null>(null);
  const btnRef = React.useRef<HTMLButtonElement>(null);
  const menuRef = React.useRef<HTMLDivElement>(null);

  // Position the menu with fixed coords anchored to the button, flipping above
  // when there isn't room below. It's rendered in a portal on document.body so
  // it can NEVER be clipped by the table's horizontal-scroll wrapper — the bug
  // where the ⋯ menu showed nothing for rows near the bottom of the viewport.
  React.useEffect(() => {
    if (!open) { setPos(null); return; }
    const place = () => {
      const b = btnRef.current?.getBoundingClientRect();
      if (!b) return;
      // First-frame estimate only — the layout effect below re-measures the
      // real menu height once it's in the DOM (so token/scale changes can't
      // strand the flip-above math).
      const menuH = menuRef.current?.getBoundingClientRect().height ?? (mayMutate ? 250 : 104);
      const openUp = window.innerHeight - b.bottom < menuH + 12;
      setPos({
        top: openUp ? Math.max(8, b.top - menuH - 4) : b.bottom + 4,
        right: Math.max(8, window.innerWidth - b.right),
      });
    };
    place();
    const close = () => setOpen(false);
    const onDocDown = (e: MouseEvent) => {
      if (btnRef.current?.contains(e.target as Node) || menuRef.current?.contains(e.target as Node)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", onDocDown);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      document.removeEventListener("mousedown", onDocDown);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [open, mayMutate]);

  // Re-measure once the portal is in the DOM: the estimate above can be off
  // after type-scale changes, which would make a flipped-up menu overlap its
  // trigger or clip at the viewport top.
  React.useLayoutEffect(() => {
    if (!open || !pos) return;
    const m = menuRef.current?.getBoundingClientRect();
    const b = btnRef.current?.getBoundingClientRect();
    if (!m || !b) return;
    const openUp = window.innerHeight - b.bottom < m.height + 12;
    const top = openUp ? Math.max(8, b.top - m.height - 4) : b.bottom + 4;
    if (Math.abs(top - pos.top) > 1) setPos({ top, right: pos.right });
  }, [open, pos]);

  const Item = ({ children, onClick, danger }: { children: React.ReactNode; onClick: () => void; danger?: boolean }) => (
    <button type="button" role="menuitem" className={"btn xs ghost" + (danger ? " danger" : "")}
      style={{ width: "100%", justifyContent: "flex-start" }}
      onClick={() => { setOpen(false); onClick(); }}>
      {children}
    </button>
  );

  return (
    <>
      <button ref={btnRef} type="button" className="btn xs ghost icon row-actions" title="Actions" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <Ico.more className="icon sm" style={{ color: "var(--text-muted)" }} />
      </button>
      {open && pos && typeof document !== "undefined" && createPortal(
        <div ref={menuRef} className="card" role="menu" style={{ position: "fixed", top: pos.top, right: pos.right, zIndex: 200, width: 160, padding: 6, boxShadow: "var(--sh-popover)", display: "flex", flexDirection: "column", gap: 2 }}>
          <a role="menuitem" className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} href={`/files/${file.id}`}><Ico.eye className="icon sm" /> Open</a>
          <Item onClick={onDownload}><Ico.download className="icon sm" /> Download</Item>
          {mayMutate && <a role="menuitem" className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} href={`/share?file=${encodeURIComponent(file.id)}`}><Ico.share className="icon sm" /> Share</a>}
          {mayMutate && <Item onClick={onRename}><Ico.cog className="icon sm" /> Rename</Item>}
          {mayMutate && (
            <>
              <div className="divider" style={{ margin: "4px 0" }} />
              <Item danger onClick={onDelete}><Ico.trash className="icon sm" /> Delete</Item>
            </>
          )}
        </div>,
        document.body,
      )}
    </>
  );
}

// #10 — bulk-bar "Move to folder" picker. Lists the active system's folders
// plus a "Root (no folder)" target; each choice PATCHes folder_id onto every
// selected file. The button sits on the dark bulk bar, but the popover uses the
// `card` class so it reads on a normal background.
function MoveMenu({ folders, busy, onMove }: { folders: Folder[]; busy: boolean; onMove: (folderId: string | null) => void }) {
  const [open, setOpen] = React.useState(false);
  const ref = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);

  const pick = (folderId: string | null) => { setOpen(false); onMove(folderId); };

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button type="button" className="btn xs" style={{ background: "transparent", color: "inherit", borderColor: "currentColor" }}
        disabled={busy} onClick={() => setOpen((v) => !v)}>
        <Ico.folder className="icon sm" /> Move
      </button>
      {open && (
        <div className="card" style={{ position: "absolute", right: 0, bottom: "calc(100% + 6px)", zIndex: 60, width: 220, maxHeight: 280, overflow: "auto", padding: 6, boxShadow: "var(--sh-popover)", display: "flex", flexDirection: "column", gap: 2 }}>
          <div className="t-xs t-subtle t-medium" style={{ padding: "4px 8px" }}>Move to folder</div>
          <button type="button" className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} onClick={() => pick(null)}>
            <Ico.home className="icon sm" /> Root (no folder)
          </button>
          {folders.length > 0 && <div className="divider" style={{ margin: "4px 0" }} />}
          {folders.map((f) => (
            <button key={f.id} type="button" className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} onClick={() => pick(f.id)}>
              <Ico.folder className="icon sm" style={{ color: f.color ?? "var(--text-muted)" }} /> {f.name}
            </button>
          ))}
          {folders.length === 0 && (
            <div className="t-xs t-subtle" style={{ padding: "4px 8px" }}>No folders here yet — use “New folder” to create one.</div>
          )}
        </div>
      )}
    </div>
  );
}
