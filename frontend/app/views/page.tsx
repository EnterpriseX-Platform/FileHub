import Link from "next/link";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeSystems, safeViews } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtAgo } from "@/lib/format";
import { hrefForView } from "@/lib/view-href";

/// Index page for saved views.  Until today this URL 404'd because we only
/// shipped `/views/new` — clicking "Views" in the sidebar worked, but
/// removing the `/new` segment (or hitting a bookmark) sent users to
/// Next.js's _not-found page.  This page lists every saved view from the
/// API and points to `/views/new` for the create flow.
export default async function ViewsIndexPage() {
  const { cookieHeader } = await loadServerCtx();
  const [views, systems] = await Promise.all([safeViews(cookieHeader), safeSystems(cookieHeader)]);

  return (
    <div className="scr">
      <Sidebar nav="views" systems={systems} />
      <TopBar
        crumbs={["Workspace", "Views"]}
        actions={<a className="btn primary" href="/views/new"><Ico.plus className="icon sm" /> New view</a>}
      />
      <div className="main" style={{ overflow: "auto", padding: "24px 32px" }}>
        <div style={{ maxWidth: 960 }}>
          <div className="t-3xl t-semibold">Saved views</div>
          <div className="t-sm t-muted" style={{ marginTop: 4, marginBottom: 24 }}>
            {views.length} view{views.length === 1 ? "" : "s"} · live from /api/views
          </div>

          {views.length === 0 ? (
            <div className="card" style={{ padding: 32, textAlign: "center" }}>
              <div className="t-md t-semibold" style={{ marginBottom: 4 }}>No views yet</div>
              <div className="t-sm t-muted" style={{ marginBottom: 16 }}>
                Save a filtered table, board, or gallery to keep coming back to it.
              </div>
              <Link className="btn primary" href="/views/new">
                <Ico.plus className="icon sm" /> Create your first view
              </Link>
            </div>
          ) : (
            <table className="tbl">
              <thead>
                <tr>
                  <th>Name</th>
                  <th style={{ width: 110 }}>Layout</th>
                  <th style={{ width: 90 }}>Pinned</th>
                  <th style={{ width: 130 }}>Created</th>
                </tr>
              </thead>
              <tbody>
                {views.map((v) => (
                  <tr key={v.id}>
                    <td>
                      <Link href={hrefForView(v.filters)} style={{ color: "var(--text)" }}>
                        <span className="t-medium">{v.name}</span>
                      </Link>
                    </td>
                    <td><Pill sm>{v.layout}</Pill></td>
                    <td>
                      {v.pinned
                        ? <Pill tone="amber" sm><Ico.pin className="icon sm" />pinned</Pill>
                        : <span className="t-subtle">—</span>}
                    </td>
                    <td className="t-sm t-muted">{fmtAgo(v.created_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </div>
  );
}
