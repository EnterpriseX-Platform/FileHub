import { notFound } from "next/navigation";

import { Ico } from "@/components/icons";
import { Av, Ft, Pill, Prop, Tag } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFile, safeSystems, safePermissions, safeOrgs } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { fmtAgo, fmtBytes, parseJsonArray, statusTone } from "@/lib/format";
import { canMutate } from "@/lib/roles";

import { FileActions } from "./actions";
import { FilePreview } from "./preview";
import { FileSidecar } from "./sidecar";

export default async function FileDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const { cookieHeader, role } = await loadServerCtx();
  const [file, systems, perms] = await Promise.all([
    safeFile(id, cookieHeader),
    safeSystems(cookieHeader),
    safePermissions(id, cookieHeader),
  ]);
  if (!file) notFound();

  const tags = parseJsonArray(file.tags);
  const sys = systems.find((s) => s.id === file.system_id);
  const orgs = await safeOrgs(file.system_id, cookieHeader);

  return (
    <div className="scr with-inspector">
      <Sidebar nav="files" systems={systems} orgs={orgs} systemActive={file.system_id} orgActive={file.org_id ?? undefined} />
      <TopBar
        crumbs={["Workspace", sys?.name ?? file.system_id, "Files"]}
        title={file.name}
        actions={
          <>
            {canMutate(role) && <a className="btn sm" href={`/share?file=${encodeURIComponent(file.id)}`}><Ico.share /> Share</a>}
            <a className="btn sm" href={`/filehub/api/files/${encodeURIComponent(file.id)}/download`}><Ico.download /> Download</a>
          </>
        }
      />

      <div className="main" style={{ background: "var(--bg-muted)", overflow: "auto" }}>
        <div style={{ padding: "20px 24px", display: "flex", flexDirection: "column", alignItems: "center", gap: 16 }}>
          <FilePreview fileId={file.id} fileType={file.file_type} fileName={file.name} />
        </div>
      </div>

      <div className="inspector">
        <div style={{ padding: "16px 16px 12px", borderBottom: "1px solid var(--border)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <Ft type={file.file_type} size="lg" />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="t-md t-semibold t-trunc">{file.name}</div>
              <div className="t-xs t-muted">
                {fmtBytes(file.size_bytes)} · v{file.version} · uploaded {fmtAgo(file.created_at)}
              </div>
            </div>
          </div>
          {canMutate(role) && <FileActions fileId={file.id} currentStatus={file.status} />}
        </div>

        <div style={{ padding: "12px 16px", flex: 1, overflow: "auto" }}>
          <Label>System fields</Label>
          <Prop label="Status" icon={<span style={{ width: 6, height: 6, background: `var(--c-${statusTone(file.status)})`, borderRadius: 3 }} />}>
            <Pill tone={statusTone(file.status)}><span className="dot" />{file.status}</Pill>
          </Prop>
          {file.project && (
            <Prop label="Project" icon={<Ico.tag className="icon sm" />}>
              <Pill tone="indigo">{file.project}</Pill>
            </Prop>
          )}
          <Prop label="Owner" icon={<Ico.user className="icon sm" />}>
            <Av name={file.owner} tone="rose" /><span>{file.owner}</span>
          </Prop>
          <Prop label="Type" icon={<Ico.layers className="icon sm" />}>
            <Pill>{file.file_type}</Pill>
          </Prop>
          <Prop label="Tags" icon={<Ico.tag className="icon sm" />}>
            {tags.length > 0 ? tags.map((t) => <Tag key={t}>{t}</Tag>) : <span className="t-subtle">—</span>}
          </Prop>
          <Prop label="Modified" icon={<Ico.clock className="icon sm" />}>
            <span>{fmtAgo(file.modified_at)}</span>
          </Prop>
          <Prop label="Created" icon={<Ico.clock className="icon sm" />}>
            <span>{fmtAgo(file.created_at)}</span>
          </Prop>

          <div className="divider" />

          <Label>Storage</Label>
          <Prop label="System" icon={<Ico.database className="icon sm" />}>
            {sys ? <Pill tone={sys.tone}><span className="dot" />{sys.name}</Pill> : <span className="t-mono">{file.system_id}</span>}
          </Prop>
          {/* Bucket/path/ETag are operator detail, not user metadata — folded
              away by default. Native <details> keeps this a server component. */}
          <details className="disclosure">
            <summary>Technical details</summary>
            <Prop label="Bucket" icon={<Ico.bucket className="icon sm" />}><span className="t-mono t-sm">{file.bucket}</span></Prop>
            <Prop label="Path"   icon={<Ico.folder className="icon sm" />}><span className="t-mono t-sm t-trunc">{file.object_key}</span></Prop>
            {file.etag && <Prop label="Checksum" icon={<Ico.tag className="icon sm" />}><span className="t-mono t-xs t-trunc">{file.etag}</span></Prop>}
            <Prop label="Encryption" icon={<Ico.shield className="icon sm" />}>
              {file.encrypted ? <Pill tone="emerald"><span className="dot" />Encrypted at rest</Pill> : <Pill>Not encrypted</Pill>}
            </Prop>
          </details>

          <div className="divider" />
          <div style={{ display: "flex", alignItems: "center", marginBottom: 8 }}>
            <Label>Permissions</Label>
            <span className="t-xs t-muted" style={{ marginLeft: "auto" }}>{perms.length}</span>
          </div>
          {perms.length === 0 ? (
            <div className="t-xs t-subtle">No explicit permissions</div>
          ) : perms.slice(0, 6).map((p) => (
            <div key={p.id} style={{ display: "flex", gap: 8, alignItems: "flex-start", padding: "6px 0" }}>
              <Av name={p.principal} tone="rose" />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="t-sm t-trunc"><span className="t-semibold">{p.principal}</span></div>
                <div className="t-xs t-subtle">{p.role} · {p.principal_type}{p.external ? " · external" : ""}</div>
              </div>
            </div>
          ))}
          {perms.length > 6 && (
            <div className="t-xs t-muted" style={{ marginTop: 6 }}>+{perms.length - 6} more</div>
          )}

          {/* Versions, workflow, and comments — fetched client-side so the
              static inspector stays cheap.  See sidecar.tsx for the layout. */}
          <FileSidecar fileId={file.id} />
        </div>
      </div>
    </div>
  );
}

function Label({ children }: { children: React.ReactNode }) {
  return (
    <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 4 }}>
      {children}
    </div>
  );
}
