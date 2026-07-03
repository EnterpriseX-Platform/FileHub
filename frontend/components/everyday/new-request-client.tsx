"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { KindTile, KIND_META, money } from "@/components/everyday/request-bits";
import type { FormSchema, IntakeResult, RequestKind, RouteStep } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { canMutate } from "@/lib/roles";

/// Start a request, chat-first. The user describes what they need; the AI picks
/// the request type, fills the form, and proposes an approval route (which the
/// user can edit). "Fill a form instead" drops straight to the manual form. On
/// submit we POST /api/requests, which starts the workflow, and jump to detail.

type Stage = "chat" | "thinking" | "draft" | "form";

const EXAMPLES = [
  "Reimburse ฿4,500 for the team offsite lunch on Tuesday, receipt attached",
  "I need a new laptop, mine keeps crashing during builds",
  "Please approve the Phattana vendor contract before Friday",
];

// A curated pool for the "edit approvers" affordance — the seed review team.
const REVIEWER_POOL: RouteStep[] = [
  { reviewer_id: "usr_krit", reviewer_name: "Krit M.", step_name: "Manager" },
  { reviewer_id: "usr_pat", reviewer_name: "Pat S.", step_name: "Finance" },
  { reviewer_id: "usr_wisanu", reviewer_name: "Wisanu T.", step_name: "Legal" },
  { reviewer_id: "usr_sarah", reviewer_name: "Sarah L.", step_name: "Director" },
  { reviewer_id: "usr_anong", reviewer_name: "Anong K.", step_name: "Approver" },
];

export function NewRequestClient({ forms, role }: { forms: FormSchema[]; role: string | null }) {
  const { t, locale } = useI18n();
  const router = useRouter();

  const [stage, setStage] = React.useState<Stage>("chat");
  const [message, setMessage] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);

  // The working draft (shared by the AI draft + the manual form).
  const [kind, setKind] = React.useState<RequestKind>("expense");
  const [title, setTitle] = React.useState("");
  const [values, setValues] = React.useState<Record<string, string>>({});
  const [aiSummary, setAiSummary] = React.useState("");
  const [route, setRoute] = React.useState<RouteStep[]>([]);
  const [attach, setAttach] = React.useState<{ id: string; name: string } | null>(null);
  const [submitting, setSubmitting] = React.useState(false);

  const schemaFor = React.useCallback(
    (k: RequestKind) => forms.find((f) => f.kind === k),
    [forms],
  );

  const setDraftFromIntake = (r: IntakeResult) => {
    setKind(r.kind);
    setTitle(r.title || "");
    const vals: Record<string, string> = {};
    const sc = schemaFor(r.kind);
    for (const f of sc?.fields ?? []) {
      const v = (r.fields as Record<string, unknown>)[f.key];
      vals[f.key] = v == null ? "" : String(v);
    }
    if (r.amount != null && vals.amount === undefined) vals.amount = String(r.amount);
    setValues(vals);
    setAiSummary(r.ai_summary || "");
    setRoute(r.route || []);
    setStage("draft");
  };

  async function runIntake(text: string) {
    const msg = text.trim();
    if (!msg) return;
    setError(null);
    setStage("thinking");
    try {
      const r = await fetch(`/filehub/api/requests/intake`, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ message: msg }),
      });
      if (!r.ok) {
        // AI off/unavailable → fall back to the manual form, no dead end.
        startForm("expense");
        return;
      }
      setDraftFromIntake((await r.json()) as IntakeResult);
    } catch {
      startForm("expense");
    }
  }

  function startForm(k: RequestKind) {
    setKind(k);
    setTitle((cur) => cur || "");
    setValues((cur) => (Object.keys(cur).length ? cur : {}));
    if (route.length === 0) setRoute(defaultRoute(k, values.amount));
    setStage("form");
  }

  function switchFormKind(k: RequestKind) {
    setKind(k);
    setRoute(defaultRoute(k, values.amount));
  }

  async function submit() {
    setSubmitting(true);
    setError(null);
    const amount = kind === "expense" && values.amount ? Number(values.amount) : null;
    try {
      const r = await fetch(`/filehub/api/requests`, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          kind,
          title: title.trim() || KIND_META[kind].labelEn,
          form_data: values,
          amount: Number.isFinite(amount as number) ? amount : null,
          reviewers: route.map((s) => ({ reviewer_id: s.reviewer_id, step_name: s.step_name })),
          order_mode: "sequential",
          ai_summary: aiSummary || null,
          file_id: attach?.id ?? null,
        }),
      });
      if (!r.ok) {
        const b = await r.json().catch(() => ({}));
        throw new Error(b.error || `could not submit (${r.status})`);
      }
      const created = await r.json();
      router.push(`/requests/${encodeURIComponent(created.id)}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : "could not submit");
      setSubmitting(false);
    }
  }

  const label = (f: { label_en: string; label_th: string }) => (locale === "th" ? f.label_th : f.label_en);

  return (
    <div className="page" style={{ maxWidth: 760 }}>
      <button className="req-back" onClick={() => router.push("/requests")}>
        <Ico.chevron className="icon sm" style={{ transform: "rotate(180deg)" }} /> {t("req.title")}
      </button>
      <div className="t-3xl t-semibold" style={{ marginTop: 6 }}>{t("req.newH")}</div>
      <div className="t-sm t-muted" style={{ marginTop: 2 }}>{t("req.newSub")}</div>

      {(stage === "chat" || stage === "thinking") && (
        <div className="req-hero" style={{ marginTop: 18 }}>
          <div className="req-asklabel">
            <span style={{ color: "var(--c-violet)", display: "inline-flex" }}><Ico.sparkle className="icon sm" /></span>
            {t("req.askLabel")}
          </div>
          <div className="req-chatbox">
            <textarea
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              placeholder={t("req.askPh")}
              rows={2}
              autoFocus
              onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) runIntake(message); }}
            />
            <div className="req-chatfoot">
              <button className="btn ghost sm" onClick={() => startForm("expense")}>{t("req.fillForm")}</button>
              <div style={{ flex: 1 }} />
              <button className="btn primary" disabled={stage === "thinking" || !message.trim()} onClick={() => runIntake(message)}>
                {t("req.continue")} <Ico.chevron className="icon sm" />
              </button>
            </div>
          </div>
          {stage === "chat" && (
            <div className="req-chips">
              {EXAMPLES.map((ex) => (
                <button key={ex} className="req-chip" onClick={() => { setMessage(ex); runIntake(ex); }}>{ex}</button>
              ))}
            </div>
          )}
          {stage === "thinking" && (
            <div className="req-thinking">
              <span style={{ display: "inline-flex", color: "var(--c-violet)" }}><Ico.sparkle className="icon sm" /></span>
              {t("req.reading")}
              <span className="req-dots"><span /><span /><span /></span>
            </div>
          )}
        </div>
      )}

      {stage === "draft" && (
        <div style={{ marginTop: 22 }}>
          <div className="req-aitag"><Ico.sparkle className="icon sm" /> {t("req.understood")}</div>
          <div className="card" style={{ overflow: "hidden", marginTop: 10 }}>
            <div className="req-drafttop">
              <KindTile kind={kind} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <input className="req-titleinput" value={title} onChange={(e) => setTitle(e.target.value)} placeholder={KIND_META[kind].labelEn} />
                <div className="t-xs t-subtle">{KIND_META[kind].labelEn} · fields extracted from your message — edit anything.</div>
              </div>
            </div>
            <FormGrid schema={schemaFor(kind)} values={values} setValues={setValues} label={label} />
          </div>

          <RouteEditor route={route} setRoute={setRoute} t={t} />

          {aiSummary && (
            <div className="req-aisum" style={{ marginTop: 16 }}>
              <div className="req-aisum-h"><Ico.sparkle className="icon sm" /> {t("req.forApprover")}</div>
              <div className="t-sm">{aiSummary}</div>
            </div>
          )}

          <AttachRow role={role} attach={attach} setAttach={setAttach} t={t} />

          {error && <div className="t-sm" style={{ color: "var(--c-rose)", marginTop: 12 }}>{error}</div>}

          <div style={{ display: "flex", gap: 10, marginTop: 18, alignItems: "center", flexWrap: "wrap" }}>
            <button className="btn primary lg" disabled={submitting} onClick={submit}>
              {submitting ? t("req.sending") : t("req.send")} {!submitting && <Ico.chevron className="icon sm" />}
            </button>
            <button className="btn lg" onClick={() => setStage("form")}>{t("req.fillForm")}</button>
            <div style={{ flex: 1 }} />
            <span className="t-xs t-subtle">Nothing is sent until you confirm.</span>
          </div>
        </div>
      )}

      {stage === "form" && (
        <div className="card pad" style={{ marginTop: 22 }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10, marginBottom: 14 }}>
            <div className="t-lg t-semibold">{t("req.new")}</div>
            <select className="req-select" value={kind} onChange={(e) => switchFormKind(e.target.value as RequestKind)}>
              {forms.map((f) => (
                <option key={f.kind} value={f.kind}>{locale === "th" ? f.name_th : f.name_en}</option>
              ))}
            </select>
          </div>
          <input className="req-titleinput" style={{ fontSize: 16, marginBottom: 10 }}
            value={title} onChange={(e) => setTitle(e.target.value)} placeholder={`${KIND_META[kind].labelEn} title`} />
          <div className="card" style={{ overflow: "hidden" }}>
            <FormGrid schema={schemaFor(kind)} values={values} setValues={setValues} label={label} />
          </div>
          <RouteEditor route={route} setRoute={setRoute} t={t} />
          <AttachRow role={role} attach={attach} setAttach={setAttach} t={t} />
          {error && <div className="t-sm" style={{ color: "var(--c-rose)", marginTop: 12 }}>{error}</div>}
          <div style={{ display: "flex", gap: 10, marginTop: 16 }}>
            <button className="btn primary lg" disabled={submitting} onClick={submit}>{submitting ? t("req.sending") : t("req.send")}</button>
            <button className="btn ghost lg" onClick={() => setStage("chat")}>{t("req.backChat")}</button>
          </div>
        </div>
      )}
    </div>
  );
}

function defaultRoute(kind: RequestKind, amountStr?: string): RouteStep[] {
  const pick = (id: string) => REVIEWER_POOL.find((r) => r.reviewer_id === id)!;
  switch (kind) {
    case "it": return [{ ...pick("usr_pat"), step_name: "IT" }, { ...pick("usr_krit"), step_name: "Asset owner" }];
    case "document": return [{ ...pick("usr_wisanu") }, { ...pick("usr_sarah") }];
    case "leave": return [{ ...pick("usr_krit") }];
    default: {
      const r: RouteStep[] = [{ ...pick("usr_krit") }, { ...pick("usr_pat") }];
      if (Number(amountStr) > 5000) r.push({ ...pick("usr_sarah") });
      return r;
    }
  }
}

function FormGrid({ schema, values, setValues, label }: {
  schema?: FormSchema;
  values: Record<string, string>;
  setValues: React.Dispatch<React.SetStateAction<Record<string, string>>>;
  label: (f: { label_en: string; label_th: string }) => string;
}) {
  if (!schema) return null;
  return (
    <div className="req-formgrid">
      {schema.fields.map((f) => {
        const full = f.kind === "textarea";
        return (
          <div key={f.key} className={"req-fld" + (full ? " full" : "")}>
            <div className="req-flabel">{label(f)}{f.required && <span style={{ color: "var(--c-rose)" }}> *</span>}</div>
            {f.kind === "textarea" ? (
              <textarea className="req-finput" rows={2} value={values[f.key] ?? ""} onChange={(e) => setValues((v) => ({ ...v, [f.key]: e.target.value }))} />
            ) : (
              <input
                className="req-finput"
                type={f.kind === "money" || f.kind === "number" ? "number" : f.kind === "date" ? "text" : "text"}
                value={values[f.key] ?? ""}
                onChange={(e) => setValues((v) => ({ ...v, [f.key]: e.target.value }))}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

function RouteEditor({ route, setRoute, t }: {
  route: RouteStep[];
  setRoute: React.Dispatch<React.SetStateAction<RouteStep[]>>;
  t: (k: string) => string;
}) {
  const [editing, setEditing] = React.useState(false);
  const canAdd = REVIEWER_POOL.filter((p) => !route.some((r) => r.reviewer_id === p.reviewer_id));
  return (
    <div className="card pad" style={{ marginTop: 16 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 12 }}>
        <div className="t-md t-semibold">{t("req.route")} <span className="t-xs t-subtle">· {t("req.byPolicy")}</span></div>
        <button className="btn ghost xs" onClick={() => setEditing((v) => !v)}>{t("req.editRoute")}</button>
      </div>
      <div className="req-route">
        {route.map((s, i) => (
          <React.Fragment key={s.reviewer_id + i}>
            <div className="req-snode">
              <span className="req-av">{s.reviewer_name.charAt(0)}</span>
              <div>
                <div className="t-xs t-semibold">{s.step_name || "Approver"}</div>
                <div className="t-xs t-subtle">{s.reviewer_name}</div>
              </div>
              {editing && route.length > 1 && (
                <button className="req-xbtn" aria-label="Remove" onClick={() => setRoute((r) => r.filter((_, j) => j !== i))}>
                  <Ico.x className="icon sm" />
                </button>
              )}
            </div>
            {i < route.length - 1 && <span className="req-slink" />}
          </React.Fragment>
        ))}
      </div>
      {editing && canAdd.length > 0 && (
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginTop: 12 }}>
          {canAdd.map((p) => (
            <button key={p.reviewer_id} className="req-chip" onClick={() => setRoute((r) => [...r, p])}>+ {p.reviewer_name}</button>
          ))}
        </div>
      )}
    </div>
  );
}

function AttachRow({ role, attach, setAttach, t }: {
  role: string | null;
  attach: { id: string; name: string } | null;
  setAttach: (a: { id: string; name: string } | null) => void;
  t: (k: string) => string;
}) {
  const [busy, setBusy] = React.useState(false);
  const inputRef = React.useRef<HTMLInputElement>(null);
  if (!canMutate(role)) return null; // attachment upload is editor+ (viewers submit form-only)

  async function onPick(file: File) {
    setBusy(true);
    try {
      const fd = new FormData();
      fd.append("file", file);
      fd.append("system_id", "sys_requests");
      const r = await fetch(`/filehub/api/files`, { method: "POST", credentials: "include", body: fd });
      if (r.ok) {
        const f = await r.json();
        setAttach({ id: f.id, name: f.name });
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={{ marginTop: 14 }}>
      <input ref={inputRef} type="file" hidden onChange={(e) => { const f = e.target.files?.[0]; if (f) onPick(f); }} />
      {attach ? (
        <div className="req-attached">
          <Ico.file className="icon" />
          <span className="t-sm t-medium t-trunc" style={{ flex: 1 }}>{attach.name}</span>
          <button className="btn ghost xs" onClick={() => setAttach(null)}><Ico.x className="icon sm" /></button>
        </div>
      ) : (
        <button className="req-attach" disabled={busy} onClick={() => inputRef.current?.click()}>
          <Ico.upload className="icon sm" /> {busy ? "…" : t("req.attach")}
        </button>
      )}
    </div>
  );
}
