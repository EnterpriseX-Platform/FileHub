"use client";

import * as React from "react";

import type { WorkspaceConfig } from "@/lib/api";

// [key, label, hint, defaultOn].
// Only policies the backend actually enforces are listed — a toggle that
// silently does nothing is worse than no toggle. The previously shown
// "planned" policies (2FA, restricted-by-default, watermarking, business
// hours) are tracked in TODO.md and come back here once they gate behaviour.
// `defaultOn` mirrors the backend default when the key is unset — external
// sharing is allowed by default, so its toggle reads ON until turned off.
const POLICY_KEYS: Array<[string, string, string, boolean]> = [
  ["allow_external_sharing", "Allow external link sharing", "When off, nobody can create new share links", true],
];

const parseBool = (s: string | undefined) => s === "true" || s === "1";

/// Editable workspace identity + access-policy panel.  Wires the previously
/// dead Save changes button to PATCH /api/workspace.  The Save button is
/// disabled until something actually changes so admins don't accidentally
/// no-op POST.
export function GeneralForm({ initial, canMutate }: { initial: WorkspaceConfig; canMutate: boolean }) {
  const [cfg, setCfg]     = React.useState<WorkspaceConfig>(initial);
  const [dirty, setDirty] = React.useState(false);
  const [busy, setBusy]   = React.useState(false);
  const [err, setErr]     = React.useState<string | null>(null);
  const [okMsg, setOk]    = React.useState<string | null>(null);

  const set = (k: string, v: string) => {
    setCfg((c) => ({ ...c, [k]: v }));
    setDirty(true);
    setOk(null);
  };

  const save = async () => {
    setBusy(true); setErr(null); setOk(null);
    try {
      const r = await fetch("/filehub/api/workspace", {
        method: "PATCH", credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ values: cfg }),
      });
      if (!r.ok) {
        const ctype = r.headers.get("content-type") ?? "";
        setErr(ctype.includes("application/json") ? ((await r.json()).error ?? r.statusText) : r.statusText);
        return;
      }
      setCfg(await r.json());
      setDirty(false); setOk("Saved");
    } finally { setBusy(false); }
  };

  return (
    <div>
      <div className="card" style={{ padding: 20, marginBottom: 16 }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
          <div className="t-md t-semibold">Workspace identity</div>
          {canMutate && (
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              {err && <span className="t-xs" style={{ color: "var(--danger)" }}>{err}</span>}
              {okMsg && <span className="t-xs" style={{ color: "var(--success)" }}>{okMsg}</span>}
              <button
                className="btn sm primary"
                onClick={save}
                disabled={!dirty || busy}
                title={dirty ? "" : "no changes to save"}
              >
                {busy ? "Saving…" : "Save changes"}
              </button>
            </div>
          )}
        </div>
        <div className="form-grid" style={{ alignItems: "center" }}>
          <div className="t-sm t-muted">Workspace name</div>
          <input className="field" style={{ width: "100%" }}
                 value={cfg.workspace_display ?? ""}
                 onChange={(e) => set("workspace_display", e.target.value)}
                 disabled={!canMutate} />

          <div className="t-sm t-muted">Tenant slug</div>
          <div className="field" style={{ width: "100%" }}>
            <span className="t-mono t-sm t-muted">filehub/</span>
            <input value={cfg.workspace_name ?? ""}
                   onChange={(e) => set("workspace_name", e.target.value)}
                   disabled={!canMutate} />
          </div>
        </div>
      </div>

      <div className="card" style={{ padding: 20, marginBottom: 16 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
          <div className="t-md t-semibold">Office documents</div>
        </div>
        <div className="t-xs t-muted" style={{ marginBottom: 14 }}>
          Which viewer opens when someone clicks a .docx / .xlsx / .pptx file.
          Collabora Online lets people edit right in the browser, but needs its
          server running and reachable at the URL below.
        </div>
        <div className="form-grid" style={{ alignItems: "center" }}>
          <div className="t-sm t-muted">Office viewer</div>
          <select
            className="field"
            value={cfg.office_viewer ?? "pdf"}
            onChange={(e) => set("office_viewer", e.target.value)}
            disabled={!canMutate}
            style={{ width: 280 }}
          >
            <option value="pdf">PDF (server-rendered)</option>
            <option value="collabora">Collabora Online (interactive editor)</option>
            <option value="disabled">Disabled (Download only)</option>
          </select>

          <div className="t-sm t-muted">Collabora URL</div>
          <input
            className="field"
            value={cfg.collabora_url ?? ""}
            onChange={(e) => set("collabora_url", e.target.value)}
            placeholder="http://localhost:9980"
            disabled={!canMutate || (cfg.office_viewer ?? "pdf") !== "collabora"}
            style={{ width: "100%" }}
          />
        </div>
      </div>

      <div className="card" style={{ padding: 20, marginBottom: 16 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
          <div className="t-md t-semibold">Access policy</div>
        </div>
        <div className="t-xs t-muted" style={{ marginBottom: 14 }}>
          Control how files can leave the workspace. Changes apply immediately.
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          {POLICY_KEYS.map(([k, label, hint, defaultOn]) => (
            <Toggle key={k}
                    label={label}
                    hint={hint}
                    on={cfg[k] === undefined ? defaultOn : parseBool(cfg[k])}
                    onChange={(v) => set(k, v ? "true" : "false")}
                    disabled={!canMutate} />
          ))}
        </div>
      </div>
    </div>
  );
}

function Toggle({ label, hint, on, onChange, disabled }: {
  label: string; hint: string; on: boolean;
  onChange: (v: boolean) => void; disabled?: boolean;
}) {
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 12, opacity: disabled ? 0.6 : 1 }}>
      <button
        type="button"
        role="switch"
        aria-label={label}
        aria-checked={on}
        onClick={() => !disabled && onChange(!on)}
        disabled={disabled}
        style={{
          width: 32, height: 18, borderRadius: 9, border: 0,
          background: on ? "var(--accent)" : "var(--bg-strong)",
          position: "relative", flexShrink: 0, cursor: disabled ? "not-allowed" : "pointer",
          padding: 0, marginTop: 2,
        }}
      >
        <span style={{
          position: "absolute", top: 2, left: on ? 16 : 2,
          width: 14, height: 14, borderRadius: 7, background: "var(--on-accent)",
          boxShadow: "var(--sh-2)", transition: "left .15s",
        }} />
      </button>
      <div style={{ flex: 1 }}>
        <div className="t-sm t-semibold">{label}</div>
        <div className="t-xs t-muted">{hint}</div>
      </div>
    </div>
  );
}
