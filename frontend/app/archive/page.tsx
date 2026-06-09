import { Ico } from "@/components/icons";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFiles, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { Ft, Pill } from "@/components/primitives";
import { fmtAgo, fmtBytes } from "@/lib/format";

export default async function ArchivePage() {
  const { cookieHeader } = await loadServerCtx();
  const [files, systems] = await Promise.all([
    safeFiles({ status: "Archived" }, cookieHeader),
    safeSystems(cookieHeader),
  ]);

  return (
    <div className="scr">
      <Sidebar nav="archive" systems={systems} />
      <TopBar crumbs={["Workspace", "Archive"]} />
      <div className="main main-pad" style={{ overflow: "auto" }}>
        <div className="page">
          <div className="t-3xl t-semibold" style={{ marginBottom: 4 }}>Archive</div>
          <div className="t-sm t-muted" style={{ marginBottom: 16 }}>
            {files.length} archived files · auto-deleted after 365 days
          </div>

          <div className="card" style={{ padding: 0 }}>
            {files.length === 0 && (
              <div style={{ padding: 32, textAlign: "center", color: "var(--text-subtle)" }}>No archived files yet — archived files are kept here for 365 days before deletion.</div>
            )}
            {files.map((f, i) => (
              <a
                key={f.id}
                href={`/files/${f.id}`}
                style={{
                  display: "flex", alignItems: "center", gap: 12,
                  padding: "12px 16px", borderTop: i ? "1px solid var(--border)" : "none",
                  color: "inherit", textDecoration: "none", cursor: "pointer",
                }}
              >
                <Ft type={f.file_type} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="t-base t-medium t-trunc">{f.name}</div>
                  <div className="t-xs t-muted">{f.bucket} · {f.object_key}</div>
                </div>
                <Pill tone="slate"><span className="dot" />Archived</Pill>
                <div className="t-sm t-mono t-muted" style={{ width: 80, textAlign: "right" }}>{fmtBytes(f.size_bytes)}</div>
                <div className="t-sm t-subtle" style={{ width: 90, textAlign: "right" }}>{fmtAgo(f.modified_at)}</div>
              </a>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
