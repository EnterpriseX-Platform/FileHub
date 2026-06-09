"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Av, Ft, Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { useAuth } from "@/lib/auth-context";
import { fmtBytes } from "@/lib/format";
import { canMutate } from "@/lib/roles";
import type { FileRow, Org, Permission, System } from "@/lib/api";

type ShareResp = {
  token: string;
  url: string;
  expires_at: string | null;
  file: FileRow;
};

export default function SharePage() {
  const [fileId, setFileId] = React.useState<string>("");
  const [file, setFile]     = React.useState<FileRow | null>(null);
  const [systems, setSystems] = React.useState<System[]>([]);
  const [orgs, setOrgs]       = React.useState<Org[]>([]);
  const [perms, setPerms]     = React.useState<Permission[]>([]);
  const [expires, setExpires] = React.useState<string>("7");
  const [note, setNote]       = React.useState<string>("");
  const [share, setShare]     = React.useState<ShareResp | null>(null);
  const [error, setError]     = React.useState<string>("");
  const [busy, setBusy]       = React.useState(false);
  const [copied, setCopied]   = React.useState(false);
  // Viewers can't create share links (backend require_role(admin|editor)), so
  // the link-options form is swapped for a read-only notice.  Reads below
  // (file lookup, existing permissions) stay visible.
  const { user: authUser, loading: authLoading } = useAuth();
  const readOnly = !canMutate(authUser?.role);

  // Read ?file= once on mount.
  React.useEffect(() => {
    const sp = new URLSearchParams(window.location.search);
    setFileId(sp.get("file") ?? "");
    fetch("/filehub/api/systems").then((r) => r.json()).then(setSystems).catch(() => {});
  }, []);

  // When file id changes, fetch the file + its existing permissions.
  React.useEffect(() => {
    if (!fileId) { setFile(null); setPerms([]); return; }
    fetch(`/filehub/api/files/${encodeURIComponent(fileId)}`)
      .then((r) => r.ok ? r.json() : Promise.reject(`${r.status}`))
      .then(setFile)
      .catch((e) => { setFile(null); setError(typeof e === "string" ? `File ${fileId}: HTTP ${e}` : ""); });
    fetch(`/filehub/api/permissions/${encodeURIComponent(fileId)}`)
      .then((r) => r.ok ? r.json() : [])
      .then(setPerms)
      .catch(() => setPerms([]));
  }, [fileId]);

  // When the file resolves, pull the orgs for its parent system so the
  // sidebar can show the right children.
  React.useEffect(() => {
    if (!file?.system_id) { setOrgs([]); return; }
    fetch(`/filehub/api/orgs?system_id=${encodeURIComponent(file.system_id)}`)
      .then((r) => r.json())
      .then((rows: Org[]) => setOrgs(rows))
      .catch(() => setOrgs([]));
  }, [file?.system_id]);

  const createShare = async () => {
    if (!fileId) return;
    setBusy(true);
    setError("");
    setShare(null);
    try {
      const body: Record<string, unknown> = {};
      const days = Number(expires);
      if (Number.isFinite(days) && days > 0) body.expires_in_days = days;
      if (note.trim()) body.note = note.trim();
      const res = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/share`, {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (res.status === 401) { window.location.href = "/login?next=" + encodeURIComponent(window.location.pathname + window.location.search); return; }
      if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
      const json = (await res.json()) as ShareResp;
      setShare(json);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const copyUrl = async () => {
    if (!share) return;
    const url = `${window.location.origin}${share.url}`;
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // no clipboard API → fall back to selection prompt
      window.prompt("Copy this URL", url);
    }
  };

  const sys = systems.find((s) => s.id === file?.system_id);

  return (
    <div className="scr">
      <Sidebar nav="share" systems={systems} orgs={orgs} systemActive={file?.system_id} />
      <TopBar
        crumbs={["Workspace", "Share"]}
      />
      <div className="main" style={{ overflow: "auto", padding: "24px 32px" }}>
        <div style={{ maxWidth: 720, margin: "0 auto" }}>
          <div className="t-2xl t-semibold" style={{ marginBottom: 4 }}>Share a file</div>
          <div className="t-sm t-muted" style={{ marginBottom: 20 }}>
            Generate a tokenized link that grants view/download without sign-in.
          </div>

          <div className="card" style={{ padding: 16, marginBottom: 16 }}>
            <Label>File</Label>
            <div className="field" style={{ width: "100%", marginBottom: 8 }}>
              <Ico.search className="icon sm" />
              <input
                value={fileId}
                onChange={(e) => setFileId(e.target.value)}
                placeholder="Paste a file id (e.g. file-001)"
                style={{ width: "100%" }}
              />
            </div>
            {file && (
              <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 0" }}>
                <Ft type={file.file_type} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="t-base t-medium t-trunc">{file.name}</div>
                  <div className="t-xs t-muted t-trunc">
                    {sys?.name ?? file.system_id} · v{file.version} · {fmtBytes(file.size_bytes)}
                    {file.encrypted ? " · encrypted" : ""}
                  </div>
                </div>
              </div>
            )}
            {error && <div className="t-xs" style={{ color: "var(--danger)", marginTop: 8 }}>{error}</div>}
          </div>

          {authLoading ? (
            <div className="card" style={{ padding: 16, marginBottom: 16 }}>
              <div className="t-sm t-subtle">Loading…</div>
            </div>
          ) : readOnly ? (
            <div className="card" style={{ padding: 16, marginBottom: 16, display: "flex", gap: 12, alignItems: "flex-start" }}>
              <div style={{ width: 36, height: 36, borderRadius: "50%", background: "var(--bg-strong)", color: "var(--text-muted)", display: "flex", alignItems: "center", justifyContent: "center", flexShrink: 0 }}>
                <Ico.lock className="icon sm" />
              </div>
              <div>
                <div className="t-sm t-semibold">Creating share links needs editor access</div>
                <div className="t-xs t-muted" style={{ marginTop: 4 }}>
                  Your role is viewer — read-only. You can see who a file is shared with below, but generating
                  new links is limited to editors and admins.
                </div>
              </div>
            </div>
          ) : (
          <div className="card" style={{ padding: 16, marginBottom: 16 }}>
            <Label>Link options</Label>
            <div className="row-2col">
              <div>
                <div className="t-xs t-subtle" style={{ marginBottom: 4 }}>Expires in (days)</div>
                <div className="field" style={{ width: "100%" }}>
                  <input
                    type="number"
                    min={0}
                    value={expires}
                    onChange={(e) => setExpires(e.target.value)}
                    placeholder="leave blank for no expiry"
                    style={{ width: "100%" }}
                  />
                </div>
              </div>
              <div>
                <div className="t-xs t-subtle" style={{ marginBottom: 4 }}>Note (optional)</div>
                <div className="field" style={{ width: "100%" }}>
                  <input
                    value={note}
                    onChange={(e) => setNote(e.target.value)}
                    placeholder="why this is shared"
                    style={{ width: "100%" }}
                  />
                </div>
              </div>
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 12 }}>
              <button className="btn primary" disabled={!file || busy} onClick={createShare}>
                <Ico.share /> {busy ? "Generating…" : "Generate link"}
              </button>
              {share && <button className="btn" onClick={() => setShare(null)}>Clear</button>}
            </div>
          </div>
          )}

          {share && (
            <div className="card" style={{ padding: 16, marginBottom: 16, borderColor: "var(--accent-border)", background: "var(--accent-soft)" }}>
              <Label>Share URL</Label>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <div className="field" style={{ flex: 1 }}>
                  <Ico.link className="icon sm" />
                  <span className="t-mono t-sm t-trunc" style={{ flex: 1 }}>
                    {typeof window !== "undefined" ? window.location.origin : ""}{share.url}
                  </span>
                </div>
                <button className="btn" onClick={copyUrl}>
                  <Ico.copy /> {copied ? "Copied!" : "Copy"}
                </button>
                <a className="btn" href={`${share.url}/download`}>
                  <Ico.download /> Try
                </a>
              </div>
              <div className="t-xs t-muted" style={{ marginTop: 8 }}>
                Token <span className="t-mono">{share.token.slice(0, 12)}…</span>
                {share.expires_at && <> · expires {new Date(share.expires_at).toLocaleString()}</>}
                {!share.expires_at && <> · no expiry</>}
              </div>
            </div>
          )}

          <div className="card" style={{ padding: 16 }}>
            <div style={{ display: "flex", alignItems: "center", marginBottom: 10 }}>
              <Label>Existing permissions</Label>
              <div className="t-xs t-muted" style={{ marginLeft: "auto" }}>{perms.length} entr{perms.length === 1 ? "y" : "ies"}</div>
            </div>
            {perms.length === 0 ? (
              <div className="t-sm t-subtle">No one has access yet — create a share link above or grant members access in Settings.</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {perms.map((p) => (
                  <div key={p.id} style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    {p.principal_type === "group" ? (
                      <div style={{ width: 22, height: 22, borderRadius: 6, background: "var(--bg-strong)", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-muted)" }}>
                        <Ico.users className="icon sm" />
                      </div>
                    ) : p.external ? (
                      <div style={{ width: 22, height: 22, borderRadius: "50%", border: "1.5px dashed var(--border-strong)", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-muted)" }}>
                        <Ico.link className="icon sm" />
                      </div>
                    ) : (
                      <Av name={p.principal} tone="rose" />
                    )}
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div className="t-sm t-semibold t-trunc">
                        {p.principal}
                        {p.external ? <Pill tone="amber" sm style={{ marginLeft: 6 }}>external</Pill> : null}
                      </div>
                      <div className="t-xs t-muted t-trunc">{p.role} · {p.principal_type}</div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function Label({ children }: { children: React.ReactNode }) {
  return (
    <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 6 }}>
      {children}
    </div>
  );
}
