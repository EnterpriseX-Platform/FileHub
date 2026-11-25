"use client";

import * as React from "react";
import { useRouter } from "next/navigation";

import { useNav } from "@/components/cx/shell";
import { Note, SkeletonRows } from "@/components/cx/ui";
import { api, when } from "@/lib/fh";

type N = { id: string; kind: string; title: string; body: string | null; link: string | null; read_at: string | null; created_at: string };

/// Notifications — shares/forwards, workflow, comments.
export default function Notifications() {
  const router = useRouter();
  const nav = useNav();
  const [rows, setRows] = React.useState<N[] | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  React.useEffect(() => { api<N[]>("/notifications").then(setRows).catch((e) => setErr(e.message)); }, []);
  const open = async (n: N) => {
    if (!n.read_at) { await api(`/notifications/${n.id}/read`, { method: "POST" }).catch(() => {}); nav.reload(); }
    let to = (n.link ?? "").replace(/^\/filehub/, "");
    // Links written before the basket screens pointed at the old explorer / file page.
    const old = /^\/explorer\?system=([^&]+)(?:&folder=([^&]+))?/.exec(to);
    if (old) to = old[2] ? `/b/${old[1]}?folder=${old[2]}` : `/b/${old[1]}`;
    const oldFile = /^\/files\/([0-9a-f-]{36})$/.exec(to);
    if (oldFile) to = `/f/${oldFile[1]}`;
    if (to.startsWith("/")) router.push(to);
  };
  return (
    <div className="cx-page" style={{ maxWidth: 960 }}>
      <div className="cx-crumbs" />
      <div className="cx-title-row"><h1 className="cx-title">Notifications</h1></div>
      {err && <Note kind="error">{err}</Note>}
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>Notification</th><th>When</th></tr></thead>
          <tbody>
            {!rows && <SkeletonRows cols={2} rows={4} />}
            {rows?.map((n) => (
              <tr key={n.id} aria-selected={!n.read_at}>
                <td className="cx-name" style={{ whiteSpace: "normal" }}>
                  <button className="cx-filelink" onClick={() => open(n)} style={{ fontWeight: n.read_at ? 400 : 600, textAlign: "left" }}><span>{n.title}</span></button>
                </td>
                <td className="cx-num">{when(n.created_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {rows && rows.length === 0 && <div className="cx-state"><strong>No notifications.</strong></div>}
      </div>
    </div>
  );
}
