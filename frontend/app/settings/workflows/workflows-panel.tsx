"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Modal } from "@/components/modal";
import { Pill } from "@/components/primitives";
import type { Member, WorkflowTemplate, WorkflowTemplateStep } from "@/lib/api";
import { fmtAgo } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

/// Template list + builder over GET/POST/DELETE /api/workflow-templates.
/// Steps are built as ordered rows (reviewer + optional step label) with
/// up/down reordering; the row order becomes the step sequence.
export function WorkflowsPanel({ initial, members, canMutate }: {
  initial: WorkflowTemplate[];
  members: Member[];
  canMutate: boolean;
}) {
  const { t } = useI18n();
  const [templates, setTemplates] = React.useState(initial);
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<WorkflowTemplate | null>(null);
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

  const save = async (body: { name: string; description?: string; order_mode: string; steps: WorkflowTemplateStep[] }, id: string | null) => {
    setBusy(true); setErr(null);
    try {
      const r = await fetch(id ? `/filehub/api/workflow-templates/${encodeURIComponent(id)}` : "/filehub/api/workflow-templates", {
        method: id ? "PATCH" : "POST", credentials: "include",
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
      setEditing(null);
      await reload();
    } finally { setBusy(false); }
  };

  return (
    <div>
      {err && <div className="t-xs" style={{ color: "var(--danger)", marginBottom: 8 }}>{err}</div>}

      {templates.length === 0 && !creating && (
        <div className="card" style={{ padding: 20, marginBottom: 16 }}>
          <div className="t-sm t-muted">{t("des.noTemplates")}</div>
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
                <button className="btn xs" disabled={busy} onClick={() => { setCreating(false); setEditing(tp); }}>{t("des.edit")}</button>
              )}
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

      {canMutate && (
        <button className="btn" onClick={() => { setEditing(null); setCreating(true); }}>
          <Ico.plus className="icon sm" /> {t("des.newTemplate")}
        </button>
      )}
      {canMutate && (creating || editing) && (
        <Modal
          title={editing ? t("des.editTemplate") : t("des.newTemplate")}
          wide
          onClose={() => { setCreating(false); setEditing(null); setErr(null); }}
        >
          <TemplateForm
            key={editing?.id ?? "new"}
            initial={editing}
            members={active}
            busy={busy}
            onSubmit={(body) => save(body, editing?.id ?? null)}
            onCancel={() => { setCreating(false); setEditing(null); }}
          />
        </Modal>
      )}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Flow builder — ActivePieces-style vertical diagram. Trigger and Done are
// fixed terminals; reviewer steps are draggable node cards with + inserts
// between them. Parallel mode fans the steps out between fork/join.
// Same create API as before: { name, description, order_mode, steps }.
// -----------------------------------------------------------------------------
type DraftStep = { reviewer_id: string; name: string };

function TemplateForm({ initial, members, busy, onSubmit, onCancel }: {
  initial: WorkflowTemplate | null;
  members: Member[];
  busy: boolean;
  onSubmit: (body: { name: string; description?: string; order_mode: string; steps: WorkflowTemplateStep[] }) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [name, setName] = React.useState(initial?.name ?? "");
  const [description, setDescription] = React.useState(initial?.description ?? "");
  const [mode, setMode] = React.useState<"sequential" | "parallel">(initial?.order_mode === "parallel" ? "parallel" : "sequential");
  const [steps, setSteps] = React.useState<DraftStep[]>(
    initial?.steps.length
      ? initial.steps.map((st) => ({ reviewer_id: st.reviewer_id, name: st.name ?? "" }))
      : [{ reviewer_id: members[0]?.id ?? "", name: "" }],
  );
  const [dragIdx, setDragIdx] = React.useState<number | null>(null);
  const [overIdx, setOverIdx] = React.useState<number | null>(null);

  const setStep = (i: number, patch: Partial<DraftStep>) =>
    setSteps((s) => s.map((st, j) => (j === i ? { ...st, ...patch } : st)));
  const insertAt = (i: number) =>
    setSteps((s) => [...s.slice(0, i), { reviewer_id: members[0]?.id ?? "", name: "" }, ...s.slice(i)]);
  const removeAt = (i: number) => setSteps((s) => s.filter((_, j) => j !== i));
  const dropOn = (target: number) => {
    if (dragIdx === null || dragIdx === target) { setDragIdx(null); setOverIdx(null); return; }
    setSteps((s) => {
      const next = [...s];
      const [moved] = next.splice(dragIdx, 1);
      next.splice(target, 0, moved);
      return next;
    });
    setDragIdx(null); setOverIdx(null);
  };

  const valid = name.trim().length > 0 && steps.length > 0 && steps.every((st) => st.reviewer_id);
  const submit = () => onSubmit({
    name: name.trim(),
    description: description.trim() || undefined,
    order_mode: mode,
    steps: steps.map((st) => ({ reviewer_id: st.reviewer_id, name: st.name.trim() || undefined })),
  });

  const stepNode = (st: DraftStep, i: number) => (
    <div
      key={i}
      className={"flow-node grab" + (overIdx === i && dragIdx !== null && dragIdx !== i ? " drop-target" : "")}
      draggable={mode === "sequential"}
      onDragStart={() => setDragIdx(i)}
      onDragOver={(e) => { e.preventDefault(); setOverIdx(i); }}
      onDragLeave={() => setOverIdx((o) => (o === i ? null : o))}
      onDrop={() => dropOn(i)}
      onDragEnd={() => { setDragIdx(null); setOverIdx(null); }}
    >
      {mode === "sequential" && <Ico.drag className="icon sm" style={{ color: "var(--text-faint)", cursor: "grab" }} />}
      <span className="fn-ic"><Ico.user className="icon sm" /></span>
      <div className="fn-body">
        <span className="fn-kind">{mode === "sequential" ? `Step ${i + 1} · review` : "Review (any order)"}</span>
        <select className="field" style={{ height: 30, width: "100%" }} value={st.reviewer_id}
          aria-label={`Step ${i + 1} reviewer`}
          onChange={(e) => setStep(i, { reviewer_id: e.target.value })}>
          {members.map((m) => <option key={m.id} value={m.id}>{m.display_name} · {m.role}</option>)}
        </select>
        <input className="field" style={{ height: 30, width: "100%" }} value={st.name}
          onChange={(e) => setStep(i, { name: e.target.value })}
          placeholder={t("des.stepLabelPh")} />
      </div>
      <button className="btn xs ghost" onClick={() => removeAt(i)} disabled={steps.length === 1}
        title="Remove step" aria-label={`Remove step ${i + 1}`}>
        <Ico.x className="icon sm" />
      </button>
    </div>
  );

  return (
    <div>
      <div style={{ display: "flex", gap: 8, marginBottom: 8, flexWrap: "wrap" }}>
        <input className="field" style={{ flex: "2 1 200px" }} value={name}
          onChange={(e) => setName(e.target.value)} placeholder={t("des.templateNamePh")} />
        <input className="field" style={{ flex: "3 1 240px" }} value={description}
          onChange={(e) => setDescription(e.target.value)} placeholder={t("des.templateDescPh")} />
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 4 }}>
        <label className="t-xs" style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <input type="radio" checked={mode === "sequential"} onChange={() => setMode("sequential")} /> {t("des.inOrder")}
        </label>
        <label className="t-xs" style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <input type="radio" checked={mode === "parallel"} onChange={() => setMode("parallel")} /> {t("des.anyOrder")}
        </label>
        {mode === "sequential" && (
          <span className="t-xs t-subtle" style={{ marginLeft: "auto" }}>{t("des.dragReorder")}</span>
        )}
      </div>

      <div className="flow">
        <div className="flow-node terminal" style={{ padding: "9px 14px" }}>
          <span className="fn-ic start"><Ico.bolt className="icon sm" /></span>
          <div className="fn-body">
            <span className="fn-kind">{t("des.trigger")}</span>
            <span className="t-sm">{t("des.triggerDesc")}</span>
          </div>
        </div>
        <div className="flow-line" />
        <button className="flow-plus" onClick={() => insertAt(0)} title="Add a step here" aria-label="Insert step at start">+</button>
        <div className="flow-line" />

        {mode === "parallel" ? (
          <div className="flow-fan">{steps.map(stepNode)}</div>
        ) : (
          steps.map((st, i) => (
            <React.Fragment key={i}>
              {i > 0 && (
                <>
                  <div className="flow-line" />
                  <button className="flow-plus" onClick={() => insertAt(i)} title="Add a step here" aria-label={`Insert step before ${i + 1}`}>+</button>
                  <div className="flow-line" />
                </>
              )}
              {stepNode(st, i)}
            </React.Fragment>
          ))
        )}

        <div className="flow-line" />
        <button className="flow-plus" onClick={() => insertAt(steps.length)} title="Add a step here" aria-label="Insert step at end">+</button>
        <div className="flow-line" />
        <div className="flow-node terminal" style={{ padding: "9px 14px" }}>
          <span className="fn-ic end"><Ico.check className="icon sm" /></span>
          <div className="fn-body">
            <span className="fn-kind">{t("des.done")}</span>
            <span className="t-sm">{t("des.doneDesc")}</span>
          </div>
        </div>
      </div>

      <div style={{ display: "flex", gap: 6, marginTop: 12 }}>
        <button className="btn primary sm" disabled={busy || !valid} onClick={submit}>{initial ? t("des.saveChanges") : t("des.createTemplate")}</button>
        <button className="btn sm" disabled={busy} onClick={onCancel}>{t("des.cancel")}</button>
      </div>
    </div>
  );
}
