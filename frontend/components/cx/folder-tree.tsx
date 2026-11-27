"use client";

import * as React from "react";
import Link from "next/link";

import type { Folder } from "@/lib/fh";
import { I } from "./ui";

/// Folder tree beside the file list (like File Explorer): every folder of the
/// basket, nested; the path to the open folder starts expanded.
export function FolderTree({ basketId, rootLabel, folders, current }: { basketId: string; rootLabel: string; folders: Folder[]; current: string | null }) {
  const byParent = React.useMemo(() => {
    const m = new Map<string | null, Folder[]>();
    for (const f of folders) {
      const k = f.parent_id ?? null;
      m.set(k, [...(m.get(k) ?? []), f]);
    }
    for (const list of m.values()) list.sort((a, b) => a.name.localeCompare(b.name));
    return m;
  }, [folders]);
  const [open, setOpen] = React.useState<Set<string>>(new Set());
  // Keep the path to the open folder expanded.
  React.useEffect(() => {
    if (!current) return;
    setOpen((prev) => {
      const next = new Set(prev);
      let cur = folders.find((f) => f.id === current);
      let guard = 0;
      while (cur?.parent_id && guard++ < 64) { next.add(cur.parent_id); cur = folders.find((f) => f.id === cur!.parent_id); }
      return next;
    });
  }, [current, folders]);
  const toggle = (id: string) => setOpen((p) => { const n = new Set(p); if (n.has(id)) n.delete(id); else n.add(id); return n; });

  const Node = ({ f, depth }: { f: Folder; depth: number }) => {
    const kids = byParent.get(f.id) ?? [];
    const isOpen = open.has(f.id);
    return (
      <li role="treeitem" aria-expanded={kids.length ? isOpen : undefined} aria-selected={current === f.id}>
        <div className="cx-ftree-row" data-current={current === f.id || undefined} style={{ paddingLeft: 8 + depth * 16 }}>
          {kids.length ? (
            <button type="button" className="cx-ftree-twisty" aria-label={isOpen ? `Collapse ${f.name}` : `Expand ${f.name}`} onClick={() => toggle(f.id)}>
              <I.chevronRight size={12} style={isOpen ? { transform: "rotate(90deg)" } : undefined} />
            </button>
          ) : <span className="cx-ftree-twisty" />}
          <Link href={`/b/${basketId}?folder=${f.id}`} className="cx-ftree-link" title={f.name}>{isOpen && kids.length ? <I.folderOpen size={16} /> : <I.folder size={16} />}<span>{f.name}</span>{f.file_count ? <small className="cx-ftree-n cx-num">{f.file_count}</small> : null}</Link>
        </div>
        {isOpen && kids.length > 0 && <ul role="group">{kids.map((k) => <Node key={k.id} f={k} depth={depth + 1} />)}</ul>}
      </li>
    );
  };

  return (
    <nav className="cx-ftree" aria-label="Folders">
      <div className="cx-ftree-head">Folders</div>
      <ul role="tree">
        <li role="treeitem" aria-selected={!current}>
          <div className="cx-ftree-row" data-current={!current || undefined} style={{ paddingLeft: 8 }}>
            <span className="cx-ftree-twisty"><I.chevronRight size={12} style={{ transform: "rotate(90deg)" }} /></span>
            <Link href={`/b/${basketId}`} className="cx-ftree-link" title={rootLabel}><I.folderOpen size={16} /><span>{rootLabel}</span></Link>
          </div>
          <ul role="group">{(byParent.get(null) ?? []).map((f) => <Node key={f.id} f={f} depth={1} />)}</ul>
        </li>
      </ul>
    </nav>
  );
}
