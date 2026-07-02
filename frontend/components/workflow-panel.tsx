"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Av, Pill } from "@/components/primitives";
import { useAuth } from "@/lib/auth-context";
import type { Member, WorkflowTemplate } from "@/lib/api";
import { fmtAgo } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { canMutate } from "@/lib/roles";

// Mirrors backend p1.rs shapes. list_workflow returns
// `Vec<(Workflow, Vec<WorkflowStep>)>` — JSON `[[workflowObj, [steps]], …]`.
type WorkflowStep = {
  id: string;
  workflow_id: string;
  sequence: number;
  reviewer_id: string | null;
  reviewer_name: string | null;
  name: string | null;
  decision: string; // 'pending' | 'approved' | 'rejected'
  decided_at: string | null;
  note: string | null;
};
type Workflow = {
  id: string;
  file_id: string;
  state: string; // Review | Approved | Rejected
  note: string | null;
  order_mode: string; // sequential | parallel
  template_id: string | null;
  created_at: string;
};
type WorkflowEntry = [Workflow, WorkflowStep[]];

/// Approval-workflow panel on the file view (both personas). Shows every
/// workflow with its step tracker, lets editors start one (ad-hoc reviewers or
/// a saved template, sequential/parallel), lets the assigned reviewer
/// approve/reject with an optional note (sequential turn guard mirrored from
/// the backend), and lets editors send a decided step back for rework.
export function WorkflowPanel({ fileId }: { fileId: string }) {
  const { user } = useAuth();
  const { t } = useI18n();
  const mutate = canMutate(user?.role ?? null);

  const [entries, setEntries] = React.useState<WorkflowEntry[] | null>(null);
  const [starting, setStarting] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);

  const base = `/filehub/api/files/${encodeURIComponent(fileId)}/workflow`;

  const load = React.useCallback(async () => {
    try {
      const r = await fetch(base, { credentials: "include", cache: "no-store" });
      if (r.ok) setEntries(await r.json());
    } catch { /* keep last */ }
  }, [base]);

  React.useEffect(() => { load(); }, [load]);

  const post = async (url: string, body?: unknown) => {
    setBusy(true); setErr(null);
    try {
      const r = await fetch(url, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body ?? {}),
      });
      if (!r.ok) {
        let detail = `HTTP ${r.status}`;
        try { detail = (await r.json()).error ?? detail; } catch { /* keep */ }
        setErr(detail);
        return false;
      }
      await load();
      return true;
    } finally { setBusy(false); }
  };

  return (
    <section style={{ marginBottom: 12 }}>
      <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 8, display: "flex", alignItems: "center", gap: 6 }}>
        <Ico.layers className="icon sm" /> {t("wf.title")}
      </div>

      {entries === null ? (
        <div className="ai-sk" style={{ height: 14, width: "70%" }} />
      ) : entries.length === 0 ? (
        <div className="t-xs t-subtle" style={{ marginBottom: 8 }}>{t("wf.none")}</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 8 }}>
          {entries.map(([wf, steps], i) => (
            <WorkflowCard
              key={wf.id}
              wf={wf}
              steps={steps}
              latest={i === 0}
              userId={user?.id ?? null}
              mutate={mutate}
              busy={busy}
              onDecide={(stepId, decision, note) =>
                post(`/filehub/api/workflow-steps/${stepId}/decision`, { decision, note: note || null })}
              onSendBack={(stepId) =>
                post(`/filehub/api/workflow-steps/${stepId}/send-back`)}
            />
          ))}
        </div>
      )}

      {err && <div className="t-xs" style={{ color: "var(--danger)", marginBottom: 6 }}>{err}</div>}

      {mutate && !starting && (
        <button className="btn xs" onClick={() => setStarting(true)}>
          <Ico.plus className="icon sm" /> {t("wf.start")}
        </button>
      )}
      {mutate && starting && (
        <StartForm
          busy={busy}
          onCancel={() => setStarting(false)}
          onStart={async (body) => {
            if (await post(base, body)) setStarting(false);
          }}
        />
      )}
    </section>
  );
}

// -----------------------------------------------------------------------------
// One workflow with its step tracker + actions
// -----------------------------------------------------------------------------
function WorkflowCard({ wf, steps, latest, userId, mutate, busy, onDecide, onSendBack }: {
  wf: Workflow;
  steps: WorkflowStep[];
  latest: boolean;
  userId: string | null;
  mutate: boolean;
  busy: boolean;
  onDecide: (stepId: string, decision: "approved" | "rejected", note: string) => void;
  onSendBack: (stepId: string) => void;
}) {
  const { t } = useI18n();
  const [note, setNote] = React.useState("");

  const active = wf.state === "Review";
  const myTurn = (s: WorkflowStep) =>
    wf.order_mode === "parallel" ||
    !steps.some((o) => o.decision === "pending" && o.sequence < s.sequence);

  const tone = wf.state === "Approved" ? "emerald" : wf.state === "Rejected" ? "rose" : "amber";

  return (
    <div style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 10 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 6 }}>
        <Pill tone={tone} sm><span className="dot" />{t("wf.st." + wf.state) || wf.state}</Pill>
        <span className="t-xs t-subtle">{t(wf.order_mode === "sequential" ? "wf.sequential" : "wf.parallel")}</span>
        <span className="t-xs t-subtle" style={{ marginLeft: "auto" }}>{fmtAgo(wf.created_at)}</span>
      </div>
      {wf.note && <div className="t-xs t-subtle" style={{ marginBottom: 6 }}>{wf.note}</div>}

      <ol style={{ listStyle: "none", padding: 0, margin: 0, display: "flex", flexDirection: "column", gap: 4 }}>
        {steps.map((s) => {
          const mine = active && mutate && s.decision === "pending" && s.reviewer_id === userId;
          return (
            <li key={s.id} style={{ padding: "4px 0", borderTop: "1px solid var(--border-subtle)" }}>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <span className="t-xs t-mono t-subtle" style={{ width: 16 }}>{s.sequence}.</span>
                <Av name={s.reviewer_name ?? "?"} tone="slate" />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="t-sm t-trunc">
                    {s.reviewer_name ?? s.reviewer_id}
                    {s.name && <span className="t-xs t-subtle"> · {s.name}</span>}
                  </div>
                  {s.note && <div className="t-xs t-subtle t-trunc">{s.note}</div>}
                </div>
                <DecisionPill decision={s.decision} />
                {latest && mutate && s.decision !== "pending" && (
                  <button
                    className="btn xs ghost"
                    disabled={busy}
                    title={t("wf.sendBack")}
                    aria-label={t("wf.sendBack")}
                    onClick={() => onSendBack(s.id)}
                  >
                    <Ico.refresh className="icon sm" />
                  </button>
                )}
              </div>
              {mine && (
                <div style={{ display: "flex", gap: 6, marginTop: 6, marginLeft: 24, flexWrap: "wrap" }}>
                  <input
                    className="field"
                    style={{ flex: "1 1 120px", minWidth: 0 }}
                    value={note}
                    onChange={(e) => setNote(e.target.value)}
                    placeholder={t("wf.decisionNote")}
                  />
                  <button className="btn xs primary" disabled={busy || !myTurn(s)}
                    onClick={() => onDecide(s.id, "approved", note.trim())}>
                    {myTurn(s) ? t("wf.approve") : t("wf.waiting")}
                  </button>
                  <button className="btn xs ghost" disabled={busy || !myTurn(s)}
                    onClick={() => onDecide(s.id, "rejected", note.trim())}>
                    {t("wf.reject")}
                  </button>
                </div>
              )}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

function DecisionPill({ decision }: { decision: string }) {
  const { t } = useI18n();
  switch (decision) {
    case "approved": return <Pill tone="emerald" sm><span className="dot" />{t("wf.approved")}</Pill>;
    case "rejected": return <Pill tone="rose" sm><span className="dot" />{t("wf.rejected")}</Pill>;
    default:         return <Pill sm>{t("wf.pending")}</Pill>;
  }
}

// -----------------------------------------------------------------------------
// Start-workflow form: saved template or ad-hoc reviewer list
// -----------------------------------------------------------------------------
function StartForm({ busy, onStart, onCancel }: {
  busy: boolean;
  onStart: (body: { template_id?: string; reviewer_ids?: string[]; order_mode?: string; note?: string }) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [templates, setTemplates] = React.useState<WorkflowTemplate[] | null>(null);
  const [members, setMembers] = React.useState<Member[]>([]);
  const [templateId, setTemplateId] = React.useState<string>("");
  const [selected, setSelected] = React.useState<string[]>([]);
  const [mode, setMode] = React.useState<"sequential" | "parallel">("sequential");
  const [note, setNote] = React.useState("");

  React.useEffect(() => {
    fetch("/filehub/api/workflow-templates", { credentials: "include" })
      .then((r) => (r.ok ? r.json() : []))
      .then(setTemplates)
      .catch(() => setTemplates([]));
    fetch("/filehub/api/users", { credentials: "include" })
      .then((r) => (r.ok ? r.json() : []))
      .then((m: Member[]) => setMembers(m.filter((x) => x.status === "active")))
      .catch(() => {});
  }, []);

  const submit = () => {
    const trimmed = note.trim();
    if (templateId) {
      onStart({ template_id: templateId, note: trimmed || undefined });
    } else {
      onStart({ reviewer_ids: selected, order_mode: mode, note: trimmed || undefined });
    }
  };

  return (
    <div style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 10 }}>
      {templates && templates.length > 0 && (
        <>
          <div className="t-xs t-medium" style={{ marginBottom: 6 }}>{t("wf.useTemplate")}</div>
          <select
            className="field"
            style={{ width: "100%", marginBottom: 8 }}
            value={templateId}
            onChange={(e) => setTemplateId(e.target.value)}
          >
            <option value="">{t("wf.noTemplate")}</option>
            {templates.map((tp) => (
              <option key={tp.id} value={tp.id}>
                {tp.name} · {tp.steps.length} {t("wf.steps")}
              </option>
            ))}
          </select>
        </>
      )}

      {!templateId && (
        <>
          <div className="t-xs t-medium" style={{ marginBottom: 6 }}>{t("wf.pickReviewers")}</div>
          <div style={{ maxHeight: 160, overflow: "auto", display: "flex", flexDirection: "column", gap: 2, marginBottom: 8 }}>
            {members.map((m) => {
              const idx = selected.indexOf(m.id);
              return (
                <label key={m.id} className="t-xs" style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}>
                  <input type="checkbox" checked={idx >= 0}
                    onChange={(e) => setSelected((s) => e.target.checked ? [...s, m.id] : s.filter((x) => x !== m.id))} />
                  {m.display_name} <span className="t-subtle">· {m.role}</span>
                  {idx >= 0 && mode === "sequential" && <span className="t-subtle" style={{ marginLeft: "auto" }}>#{idx + 1}</span>}
                </label>
              );
            })}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8 }}>
            <label className="t-xs" style={{ display: "flex", gap: 4, alignItems: "center" }}>
              <input type="radio" checked={mode === "sequential"} onChange={() => setMode("sequential")} /> {t("wf.sequential")}
            </label>
            <label className="t-xs" style={{ display: "flex", gap: 4, alignItems: "center" }}>
              <input type="radio" checked={mode === "parallel"} onChange={() => setMode("parallel")} /> {t("wf.parallel")}
            </label>
          </div>
        </>
      )}

      <input
        className="field"
        style={{ width: "100%", marginBottom: 8 }}
        value={note}
        onChange={(e) => setNote(e.target.value)}
        placeholder={t("wf.noteHint")}
      />

      <div style={{ display: "flex", gap: 6 }}>
        <button className="btn xs primary" disabled={busy || (!templateId && !selected.length)} onClick={submit}>
          {t("wf.send")}
        </button>
        <button className="btn xs ghost" disabled={busy} onClick={onCancel}>{t("wf.cancel")}</button>
      </div>
    </div>
  );
}
