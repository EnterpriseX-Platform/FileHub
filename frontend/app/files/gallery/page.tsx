import Link from "next/link";

import { Ico } from "@/components/icons";
import { Av, Ft, Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFiles, safeOrgs, safeSearch, safeStats, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtAgo, fmtBytes } from "@/lib/format";
import { canMutate } from "@/lib/roles";
import type { FileRow } from "@/lib/api";

import { ViewFilterBar } from "../view-filter-bar";
import { ViewTabs } from "../view-tabs";
import { buildViewHref, fileMatchesFilters, readViewParams } from "../view-params";

function PreviewBg({ ft, fileId }: { ft: string; fileId: string }) {
  if (ft === "img" || ft === "jpg" || ft === "png") {
    // Real image — the download endpoint streams plaintext bytes thanks to the
    // storage decryption layer, so the browser can render it directly.
    return (
      // next/image is wrong here: the optimizer refetches server-side without
      // the session cookie, so the authenticated download endpoint would 401.
      // eslint-disable-next-line @next/next/no-img-element
      <img
        src={`/filehub/api/files/${fileId}/download`}
        alt=""
        style={{ width: "100%", height: "100%", objectFit: "cover" }}
      />
    );
  }
  return <Ft type={ft} size="xl" />;
}

type GalleryProps = { searchParams?: Promise<Record<string, string | string[] | undefined>> };

function bucketByDay(files: FileRow[]) {
  const today: FileRow[] = [];
  const earlier: FileRow[] = [];
  const now = Date.now();
  const oneDay = 24 * 60 * 60 * 1000;
  for (const f of files) {
    if (now - new Date(f.modified_at).getTime() < oneDay) today.push(f);
    else earlier.push(f);
  }
  return { today, earlier };
}

export default async function FilesGalleryPage({ searchParams }: GalleryProps) {
  const sp = (await searchParams) ?? {};
  const filters: Record<string, string> = {};
  for (const k of ["system_id", "org_id", "status", "project", "owner", "folder_id"] as const) {
    const v = sp[k];
    if (typeof v === "string" && v.length) filters[k] = v;
  }
  const searchTerm = typeof sp.q === "string" ? sp.q.trim() : "";
  const viewParams = readViewParams(sp);
  const { cookieHeader, role } = await loadServerCtx();
  const [files, systems, stats] = await Promise.all([
    searchTerm ? safeSearch(searchTerm, cookieHeader) : safeFiles(filters, cookieHeader),
    safeSystems(cookieHeader),
    safeStats(cookieHeader),
  ]);
  const rows = files.filter((f) => fileMatchesFilters(f, filters));
  const sys = filters.system_id ? systems.find((s) => s.id === filters.system_id) : undefined;
  const orgs = (sys ?? systems[0]) ? await safeOrgs((sys ?? systems[0]).id, cookieHeader) : [];
  const { today, earlier } = bucketByDay(rows);
  const title = sys ? `${sys.name} gallery` : "All files";

  return (
    <div className="scr">
      <Sidebar nav="files" systems={systems} stats={stats} orgs={orgs} systemActive={filters.system_id ?? systems[0]?.id} />
      <TopBar
        crumbs={sys ? ["Workspace", sys.name, "Gallery"] : ["Workspace", "Gallery"]}
      />
      <div className="main">
        <div style={{ padding: "var(--sp-3) var(--sp-6) 8px", borderBottom: "1px solid var(--border)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12 }}>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span className="t-2xl t-semibold">{title}</span>
                {sys && <Pill tone={sys.tone}><span className="dot" />{sys.name}</Pill>}
                <Pill>{rows.length} file{rows.length === 1 ? "" : "s"}</Pill>
              </div>
              <div className="t-sm t-muted" style={{ marginTop: 2 }}>Gallery view · sorted by modified date</div>
            </div>
            {canMutate(role) && (
              <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
                <Link className="btn primary" href={buildViewHref("/upload", { system_id: filters.system_id, org_id: filters.org_id })}><Ico.upload /> Upload</Link>
              </div>
            )}
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
            <ViewTabs params={viewParams} active="gallery" />
            <ViewFilterBar params={viewParams} base="/files/gallery" />
          </div>
        </div>

        <div style={{ flex: 1, overflow: "auto", padding: "var(--sp-4) var(--sp-6)" }}>
          <Section label="Today" items={today} />
          {earlier.length > 0 && <Section label="Earlier" items={earlier} faded />}
          {rows.length === 0 && (
            <div style={{ padding: 40, textAlign: "center", color: "var(--text-subtle)" }}>
              No files match this view.{canMutate(role) && <> <a href="/upload" style={{ color: "var(--accent)" }}>Upload a file</a>.</>}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function Section({ label, items, faded }: { label: string; items: FileRow[]; faded?: boolean }) {
  if (items.length === 0) return null;
  return (
    <>
      <div style={{ margin: "0 0 12px", display: "flex", alignItems: "center", gap: 8 }}>
        <span className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase" }}>{label}</span>
        <div style={{ flex: 1, height: 1, background: "var(--border)" }} />
        <span className="t-xs t-subtle">{items.length}</span>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))", gap: 14, opacity: faded ? 0.85 : 1, marginBottom: 20 }}>
        {items.map((f) => (
          <a key={f.id} href={`/files/${f.id}`} className="card" style={{ padding: 0, overflow: "hidden", cursor: "pointer", color: "inherit", textDecoration: "none" }}>
            <div style={{ aspectRatio: "4/3", background: "var(--bg-muted)", position: "relative", overflow: "hidden", display: "flex", alignItems: "center", justifyContent: "center" }}>
              <PreviewBg ft={f.file_type} fileId={f.id} />
              <div style={{ position: "absolute", top: 8, left: 8 }}><Ft type={f.file_type} /></div>
            </div>
            <div style={{ padding: 10 }}>
              <div className="t-sm t-semibold t-trunc">{f.name}</div>
              <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 6 }}>
                <Av name={f.owner} />
                <span className="t-xs t-mono t-muted">{f.file_type === "fold" ? "—" : fmtBytes(f.size_bytes)}</span>
                <span className="t-xs t-muted" style={{ marginLeft: "auto" }}>{fmtAgo(f.modified_at)}</span>
              </div>
            </div>
          </a>
        ))}
      </div>
    </>
  );
}
