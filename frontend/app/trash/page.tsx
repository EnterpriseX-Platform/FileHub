import { Ico } from "@/components/icons";
import { Pager } from "@/components/pager";
import { Av, Ft, Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFiles, safeStats, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtAgo, fmtBytes } from "@/lib/format";
import type { FileRow } from "@/lib/api";

import { TrashRowActions } from "./row-actions";

// /api/trash is private + has no helper in lib/api.ts, so the cookie has to be
// forwarded inline.  Force IPv4 to dodge the macOS ::1 vs 0.0.0.0 mismatch
// (see lib/api.ts).
async function safeTrash(cookieHeader: string): Promise<FileRow[]> {
  try {
    const r = await fetch(
      `${process.env.BACKEND_URL || "http://127.0.0.1:8090"}/fh/api/trash`,
      {
        cache: "no-store",
        headers: cookieHeader ? { cookie: cookieHeader } : undefined,
      },
    );
    if (!r.ok) return [];
    return (await r.json()) as FileRow[];
  } catch {
    return [];
  }
}

export default async function TrashPage({ searchParams }: { searchParams?: Promise<Record<string, string | string[] | undefined>> }) {
  const sp = (await searchParams) ?? {};
  const { cookieHeader, role } = await loadServerCtx();
  const [trash, systems, stats] = await Promise.all([
    safeTrash(cookieHeader),
    safeSystems(cookieHeader),
    safeStats(cookieHeader),
  ]);
  void safeFiles;

  const PAGE_SIZE = 50;
  const total = trash.length;
  const pageCount = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const page = Math.min(Math.max(0, parseInt(typeof sp.page === "string" ? sp.page : "0", 10) || 0), pageCount - 1);
  const pageRows = trash.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  return (
    <div className="scr">
      <Sidebar nav="trash" systems={systems} stats={stats} />
      <TopBar
        crumbs={["Workspace", "Trash"]}
        actions={<></>}
      />
      <div className="main main-pad" style={{ overflow: "auto" }}>
        <div className="page">
        <div className="t-2xl t-semibold" style={{ marginBottom: 4 }}>Trash</div>
        <div className="t-sm t-muted" style={{ marginBottom: 20 }}>
          {trash.length === 0
            ? "Your trash is empty — deleted files rest here for 30 days before they're purged."
            : `${trash.length} deleted file${trash.length === 1 ? "" : "s"} · admins can hard-delete; editors can restore.`}
        </div>

        {trash.length > 0 && (
          <div className="table-scroll">
          <table className="tbl">
            <thead>
              <tr>
                <th>Name</th>
                <th style={{ width: 110 }}>Type</th>
                <th style={{ width: 110 }}>System</th>
                <th style={{ width: 140 }}>Owner</th>
                <th style={{ width: 90, textAlign: "right" }}>Size</th>
                <th style={{ width: 140 }}>Deleted</th>
                <th style={{ width: 220 }} />
              </tr>
            </thead>
            <tbody>
              {pageRows.map((f) => {
                const sys = systems.find((s) => s.id === f.system_id);
                return (
                  <tr key={f.id}>
                    <td>
                      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                        <Ft type={f.file_type} />
                        <span className="t-medium">{f.name}</span>
                      </div>
                    </td>
                    <td><Pill>{f.file_type}</Pill></td>
                    <td>{sys ? <Pill tone={sys.tone}><span className="dot" />{sys.name}</Pill> : <span className="t-mono t-xs t-subtle">{f.system_id.slice(0, 8)}</span>}</td>
                    <td>
                      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        <Av name={f.owner} /><span className="t-sm">{f.owner}</span>
                      </div>
                    </td>
                    <td className="t-mono t-sm t-tabular t-muted" style={{ textAlign: "right" }}>{fmtBytes(f.size_bytes)}</td>
                    <td className="t-sm t-muted">{fmtAgo(f.modified_at)}</td>
                    <td><TrashRowActions fileId={f.id} fileName={f.name} role={role} /></td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          </div>
        )}
        <Pager page={page} pageSize={PAGE_SIZE} total={total} hrefFor={(p) => (p === 0 ? "/trash" : `/trash?page=${p}`)} />
        </div>
      </div>
    </div>
  );
}
