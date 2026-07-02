"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import type { Member, WorkflowTemplate, WorkflowTemplateStep } from "@/lib/api";
import { fmtAgo } from "@/lib/format";

/// Template list + builder over GET/POST/DELETE /api/workflow-templates.
/// Steps are built as ordered rows (reviewer + optional step label) with
/// up/down reordering; the row order becomes the step sequence.
export function WorkflowsPanel({ initial, members, canMutate }: {
  initial: WorkflowTemplate[];
  members: Member[];
  canMutate: boolean;
}) {
  const [templates, setTemplates] = React.useState(initial);
  const [creating, setCreating] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);

  const active = members.filter((m) => m.status === "active");
  const nameOf = (id: string) => members.find((m) => m.id === id)?.display_name ?? id;

  const reload = async () => {
    try {
      const r = await fetch("/filehub/api/workflow-templates", { credentials: "include", cache: "no-store" });
      if (r.ok) setTemplates(await r.json());
    } catch { /* keep last */ }
  };

  const remove = async (id: string) => {
    setBusy(true); setErr(null);
    try {
      const r = await fetch(`/filehub/api/workflow-templates/${encodeURIComponent(id)}`, {
        method: "DELETE", credentials: "include",
      });
      if (!r.ok) { setErr(`HTTP ${r.status}`); return; }
      await reload();
    } finally { setBusy(false); }
  };

  const create = async (body: { name: string; description?: string; order_mode: string; steps: WorkflowTemplateStep[] }) => {
    setBusy(true); setErr(null);
    try {
      const r = await fetch("/filehub/api/workflow-templates", {
        method: "POST", credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!r.ok) {
        let detail = `HTTP ${r.status}`;
        try { detail = (await r.json()).error ?? detail; } catch { /* keep */ }
        setErr(detail);
        return;
      }
      setCreating(false);
      await reload();
    } finally { setBusy(false); }
  };

  return (
    <div>
      {err && <div className="t-xs" style={{ color: "var(--danger)", marginBottom: 8 }}>{err}</div>}

      {templates.length === 0 && !creating && (
        <div className="card" style={{ padding: 20, marginBottom: 16 }}>
          <div className="t-sm t-muted">
            No templates yet. A template is a saved approval route — e.g. section head → division director → deputy governor —
            that editors reuse from any file&apos;s workflow panel.
          </div>
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 16 }}>
        {templates.map((tp) => (
          <div key={tp.id} className="card" style={{ padding: 14 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <div className="t-md t-semibold" style={{ flex: 1, minWidth: 0 }}>{tp.name}</div>
              <Pill sm>{tp.order_mode === "parallel" ? "any order" : "in order"}</Pill>
              <span className="t-xs t-subtle">{fmtAgo(tp.created_at)}</span>
              {canMutate && (
                <button className="btn xs ghost" disabled={busy} onClick={() => remove(tp.id)}
                  title="Delete template" aria-label={`Delete template ${tp.name}`}>
                  <Ico.trash className="icon sm" />
                </button>
              )}
            </div>
            {tp.description && <div className="t-sm t-muted" style={{ marginTop: 4 }}>{tp.description}</div>}
            <ol style={{ listStyle: "none", padding: 0, margin: "8px 0 0", display: "flex", flexDirection: "column", gap: 4 }}>
              {tp.steps.map((st, i) => (
                <li key={i} className="t-sm" style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <span className="t-xs t-mono t-subtle" style={{ width: 16 }}>{i + 1}.</span>
                  <span>{nameOf(st.reviewer_id)}</span>
                  {st.name && <span className="t-xs t-subtle">· {st.name}</span>}
                </li>
              ))}
            </ol>
          </div>
        ))}
      </div>

      {canMutate && !creating && (
        <button className="btn" onClick={() => setCreating(true)}>
          <Ico.plus className="icon sm" /> New template
        </button>
      )}
      {canMutate && creating && (
        <TemplateForm members={active} busy={busy} onCreate={create} onCancel={() => setCreating(false)} />
      )}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Builder form
// -----------------------------------------------------------------------------
type DraftStep = { reviewer_id: string; name: string };

function TemplateForm({ members, busy, onCreate, onCancel }: {
  members: Member[];
  busy: boolean;
  onCreate: (body: { name: string; description?: string; order_mode: string; steps: WorkflowTemplateStep[] }) => void;
  onCancel: () => void;
}) {
  const [name, setName] = React.useState("");
  const [description, setDescription] = React.useState("");
  const [mode, setMode] = React.useState<"sequential" | "parallel">("sequential");
  const [steps, setSteps] = React.useState<DraftStep[]>([{ reviewer_id: members[0]?.id ?? "", name: "" }]);

  const setStep = (i: number, patch: Partial<DraftStep>) =>
    setSteps((s) => s.map((st, j) => (j === i ? { ...st, ...patch } : st)));
  const move = (i: number, dir: -1 | 1) =>
    setSteps((s) => {
      const j = i + dir;
      if (j < 0 || j >= s.length) return s;
      const next = [...s];
      [next[i], next[j]] = [next[j], next[i]];
      return next;
    });

  const valid = name.trim().length > 0 && steps.length > 0 && steps.every((st) => st.reviewer_id);

  const submit = () => onCreate({
    name: name.trim(),
    description: description.trim() || undefined,
    order_mode: mode,
    steps: steps.map((st) => ({ reviewer_id: st.reviewer_id, name: st.name.trim() || undefined })),
  });

  return (
    <div className="card" style={{ padding: 14 }}>
      <div className="t-sm t-semibold" style={{ marginBottom: 10 }}>New template</div>

      <div style={{ display: "flex", gap: 8, marginBottom: 8, flexWrap: "wrap" }}>
        <input className="field" style={{ flex: "2 1 200px" }} value={name}
          onChange={(e) => setName(e.target.value)} placeholder="Template name (e.g. Contract approval)" />
        <input className="field" style={{ flex: "3 1 240px" }} value={description}
          onChange={(e) => setDescription(e.target.value)} placeholder="Description (optional)" />
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 10 }}>
        <label className="t-xs" style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <input type="radio" checked={mode === "sequential"} onChange={() => setMode("sequential")} /> In order
        </label>
        <label className="t-xs" style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <input type="radio" checked={mode === "parallel"} onChange={() => setMode("parallel")} /> Any order
        </label>
      </div>

      <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 6 }}>
        Steps
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 10 }}>
        {steps.map((st, i) => (
          <div key={i} style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <span className="t-xs t-mono t-subtle" style={{ width: 16 }}>{i + 1}.</span>
            <select className="field" style={{ flex: "2 1 140px" }} value={st.reviewer_id}
              aria-label={`Step ${i + 1} reviewer`}
              onChange={(e) => setStep(i, { reviewer_id: e.target.value })}>
              {members.map((m) => <option key={m.id} value={m.id}>{m.display_name} · {m.role}</option>)}
            </select>
            <input className="field" style={{ flex: "2 1 140px" }} value={st.name}
              onChange={(e) => setStep(i, { name: e.target.value })}
              placeholder="Step label (e.g. Section head)" />
            <button className="btn xs ghost" onClick={() => move(i, -1)} disabled={i === 0} title="Move up" aria-label={`Move step ${i + 1} up`}>
              <Ico.up className="icon sm" />
            </button>
            <button className="btn xs ghost" onClick={() => move(i, 1)} disabled={i === steps.length - 1} title="Move down" aria-label={`Move step ${i + 1} down`}>
              <Ico.down className="icon sm" />
            </button>
            <button className="btn xs ghost" onClick={() => setSteps((s) => s.filter((_, j) => j !== i))}
              disabled={steps.length === 1} title="Remove step" aria-label={`Remove step ${i + 1}`}>
              <Ico.x className="icon sm" />
            </button>
          </div>
        ))}
      </div>
      <button className="btn xs" style={{ marginBottom: 12 }}
        onClick={() => setSteps((s) => [...s, { reviewer_id: members[0]?.id ?? "", name: "" }])}>
        <Ico.plus className="icon sm" /> Add step
      </button>

      <div style={{ display: "flex", gap: 6 }}>
        <button className="btn primary sm" disabled={busy || !valid} onClick={submit}>Create template</button>
        <button className="btn sm" disabled={busy} onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}
