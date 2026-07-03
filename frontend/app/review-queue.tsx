"use client";

import Link from "next/link";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Ft, SectionHd } from "@/components/primitives";
import { useToast } from "@/components/toast";

type Item = {
  id: string;
  name: string;
  file_type: string;
  owner: string;
  systemName: string;
};

/// Dashboard "Needs your review" queue — the console-prototype behavior:
/// Approve acts inline (real `PATCH status=Approved`), the row slides out and
/// the count decrements; Review still deep-links into the file. Hidden when
/// everything is handled.
export function ReviewQueue({ items, mayMutate }: { items: Item[]; mayMutate: boolean }) {
  const { toast } = useToast();
  const [gone, setGone] = React.useState<Set<string>>(new Set());
  const [leaving, setLeaving] = React.useState<Set<string>>(new Set());
  const [busy, setBusy] = React.useState(false);

  const live = items.filter((f) => !gone.has(f.id));
  if (live.length === 0) return null;

  const approve = async (f: Item) => {
    setBusy(true);
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(f.id)}`, {
        method: "PATCH",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ status: "Approved" }),
      });
      if (!r.ok) {
        toast(`Approve failed (HTTP ${r.status})`, "error");
        return;
      }
      setLeaving((s) => new Set(s).add(f.id));
      setTimeout(() => setGone((s) => new Set(s).add(f.id)), 320);
      toast(`${f.name} approved`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card" style={{ padding: 16, marginBottom: 16 }}>
      <SectionHd
        title="Needs your review"
        sub={`${live.length} item${live.length === 1 ? "" : "s"}`}
        action={<Link className="btn xs ghost" href="/files?status=Review">View all <Ico.chevron className="icon sm" /></Link>}
      />
      {live.map((f, i) => (
        <div
          key={f.id}
          className={leaving.has(f.id) ? "rq-row rq-gone" : "rq-row"}
          style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 0", borderTop: i ? "1px solid var(--border-subtle)" : "none" }}
        >
          <Ft type={f.file_type} />
          <div style={{ flex: 1, minWidth: 0 }}>
            <a href={`/files/${f.id}`} className="t-base t-medium t-trunc" style={{ color: "var(--text)", display: "block" }}>{f.name}</a>
            <div className="t-xs t-muted t-trunc">{f.systemName} · {f.owner}</div>
          </div>
          {mayMutate && (
            <button className="lnk" disabled={busy} onClick={() => approve(f)}>Approve →</button>
          )}
          <a className="lnk" href={`/files/${f.id}`}>Review →</a>
        </div>
      ))}
    </div>
  );
}
