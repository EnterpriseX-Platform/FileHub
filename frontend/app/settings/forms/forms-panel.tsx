"use client";

import Link from "next/link";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { KindGlyph, KindTile, colorVar, FORM_COLORS, GLYPHS } from "@/components/everyday/request-bits";
import type { FormAdmin, FormField, WorkflowTemplate } from "@/lib/api";

/// The Form Designer — list + builder over GET/POST/PATCH/DELETE /api/forms.
/// A form is a request type: metadata (name, icon, color), an ordered set of
/// fields, and the workflow template that routes it for approval. The everyday
/// "New request" flow renders these live and the AI classifies into them.
const FIELD_KINDS = ["text", "textarea", "number", "money", "date"] as const;

export function FormsPanel({ initial, templates, canMutate }: {
  initial: FormAdmin[];
  templates: WorkflowTemplate[];
  canMutate: boolean;
}) {
  const [forms, setForms] = React.useState(initial);
  const [editing, setEditing] = React.useState<FormAdmin | "new" | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);

  const templateName = (id: string | null) => templates.find((t) => t.id === id)?.name ?? "—";

  const reload = async () => {
    try {
      const r = await fetch("/filehub/api/forms", { credentials: "include", cache: "no-store" });
      if (r.ok) setForms(await r.json());
    } catch { /* keep last */ }
  };

  const save = async (body: FormBody, id: string | null) => {
    setBusy(true); setErr(null);
    try {
      const r = await fetch(id ? `/filehub/api/forms/${encodeURIComponent(id)}` : "/filehub/api/forms", {
        method: id ? "PATCH" : "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!r.ok) {
        let detail = `HTTP ${r.status}`;
        try { detail = (await r.json()).error ?? detail; } catch { /* keep */ }
        setErr(detail); return;
      }
      setEditing(null);
      await reload();
    } finally { setBusy(false); }
  };

  const remove = async (id: string) => {
    setBusy(true); setErr(null);
    try {
      const r = await fetch(`/filehub/api/forms/${encodeURIComponent(id)}`, { method: "DELETE", credentials: "include" });
      if (!r.ok) { setErr(`HTTP ${r.status}`); return; }
      await reload();
    } finally { setBusy(false); }
  };

  if (editing) {
    return (
      <FormBuilder
        initial={editing === "new" ? null : editing}
        templates={templates}
        busy={busy}
        err={err}
        onSave={(body) => save(body, editing === "new" ? null : editing.id)}
        onCancel={() => { setEditing(null); setErr(null); }}
      />
    );
  }

  return (
    <div>
      {err && <div className="t-xs" style={{ color: "var(--danger)", marginBottom: 8 }}>{err}</div>}

      <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 16 }}>
        {forms.map((f) => (
          <div key={f.id} className="card" style={{ padding: 14, display: "flex", alignItems: "center", gap: 12 }}>
            <KindTile icon={f.icon} color={f.color} />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span className="t-md t-semibold t-trunc">{f.name_en}</span>
                {!f.active && <Pill sm tone="slate">hidden</Pill>}
              </div>
              <div className="t-xs t-subtle" style={{ marginTop: 2 }}>
                {f.fields.length} field{f.fields.length === 1 ? "" : "s"} · route: {templateName(f.template_id)}
              </div>
            </div>
            {canMutate && (
              <>
                <button className="btn xs" onClick={() => setEditing(f)}>Edit</button>
                <button className="btn xs ghost" disabled={busy} onClick={() => remove(f.id)}
                  title={`Delete ${f.name_en}`} aria-label={`Delete ${f.name_en}`}>
                  <Ico.trash className="icon sm" />
                </button>
              </>
            )}
          </div>
        ))}
        {forms.length === 0 && (
          <div className="card" style={{ padding: 20 }}>
            <div className="t-sm t-muted">No request forms yet. Create one to let people submit that kind of request.</div>
          </div>
        )}
      </div>

      {canMutate && (
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <button className="btn primary" onClick={() => setEditing("new")}>
            <Ico.plus className="icon sm" /> New form
          </button>
          <Link href="/settings/workflows" className="btn ghost">
            <Ico.layers className="icon sm" /> Design approval routes
          </Link>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------
type FieldRow = { label_en: string; label_th: string; kind: string; required: boolean };
type FormBody = {
  id?: string;
  name_en: string; name_th: string; description: string | null;
  icon: string; color: string; fields: FormField[];
  template_id: string | null; active: boolean; sort_order: number;
};

function slug(s: string): string {
  return s.toLowerCase().trim().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "field";
}

function FormBuilder({ initial, templates, busy, err, onSave, onCancel }: {
  initial: FormAdmin | null;
  templates: WorkflowTemplate[];
  busy: boolean;
  err: string | null;
  onSave: (body: FormBody) => void;
  onCancel: () => void;
}) {
  const [nameEn, setNameEn] = React.useState(initial?.name_en ?? "");
  const [nameTh, setNameTh] = React.useState(initial?.name_th ?? "");
  const [description, setDescription] = React.useState(initial?.description ?? "");
  const [icon, setIcon] = React.useState(initial?.icon ?? "generic");
  const [color, setColor] = React.useState(initial?.color ?? "indigo");
  const [templateId, setTemplateId] = React.useState(initial?.template_id ?? (templates[0]?.id ?? ""));
  const [active, setActive] = React.useState(initial?.active ?? true);
  const [fields, setFields] = React.useState<FieldRow[]>(
    initial?.fields.length
      ? initial.fields.map((f) => ({ label_en: f.label_en, label_th: f.label_th, kind: f.kind, required: f.required }))
      : [{ label_en: "", label_th: "", kind: "text", required: true }],
  );

  const setField = (i: number, patch: Partial<FieldRow>) =>
    setFields((s) => s.map((f, j) => (j === i ? { ...f, ...patch } : f)));
  const addField = () => setFields((s) => [...s, { label_en: "", label_th: "", kind: "text", required: false }]);
  const removeField = (i: number) => setFields((s) => s.filter((_, j) => j !== i));
  const move = (i: number, dir: -1 | 1) => setFields((s) => {
    const j = i + dir;
    if (j < 0 || j >= s.length) return s;
    const next = [...s];
    [next[i], next[j]] = [next[j], next[i]];
    return next;
  });

  const valid = nameEn.trim().length > 0 && fields.every((f) => f.label_en.trim().length > 0);

  const submit = () => {
    // Derive stable field keys from the English label, de-duplicated.
    const seen = new Map<string, number>();
    const outFields: FormField[] = fields.map((f) => {
      let key = slug(f.label_en);
      const n = seen.get(key) ?? 0;
      seen.set(key, n + 1);
      if (n > 0) key = `${key}_${n + 1}`;
      return { key, label_en: f.label_en.trim(), label_th: f.label_th.trim() || f.label_en.trim(), kind: f.kind, required: f.required };
    });
    onSave({
      id: initial?.id,
      name_en: nameEn.trim(),
      name_th: nameTh.trim() || nameEn.trim(),
      description: description.trim() || null,
      icon, color, fields: outFields,
      template_id: templateId || null,
      active,
      sort_order: initial?.sort_order ?? 0,
    });
  };

  return (
    <div className="card" style={{ padding: 18 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 14 }}>
        <KindTile icon={icon} color={color} />
        <div className="t-lg t-semibold">{initial ? "Edit form" : "New form"}</div>
        <div style={{ flex: 1 }} />
        <label className="t-xs" style={{ display: "flex", gap: 5, alignItems: "center" }}>
          <input type="checkbox" checked={active} onChange={(e) => setActive(e.target.checked)} /> Visible in everyday
        </label>
      </div>

      {err && <div className="t-xs" style={{ color: "var(--danger)", marginBottom: 8 }}>{err}</div>}

      <div style={{ display: "flex", gap: 8, marginBottom: 10, flexWrap: "wrap" }}>
        <input className="field" style={{ flex: "2 1 220px" }} value={nameEn}
          onChange={(e) => setNameEn(e.target.value)} placeholder="Form name (e.g. Travel request)" />
        <input className="field" style={{ flex: "2 1 200px" }} value={nameTh}
          onChange={(e) => setNameTh(e.target.value)} placeholder="ชื่อฟอร์ม (Thai, optional)" />
      </div>
      <input className="field" style={{ width: "100%", marginBottom: 12 }} value={description}
        onChange={(e) => setDescription(e.target.value)} placeholder="Short description (optional)" />

      {/* Icon + color */}
      <div style={{ display: "flex", gap: 24, marginBottom: 14, flexWrap: "wrap" }}>
        <div>
          <div className="req-flabel" style={{ marginBottom: 6 }}>Icon</div>
          <div style={{ display: "flex", gap: 6 }}>
            {GLYPHS.map((g) => (
              <button key={g} onClick={() => setIcon(g)} title={g} aria-label={`Icon ${g}`}
                className="form-swatch" style={{ borderColor: icon === g ? "var(--accent)" : "var(--border)", color: "var(--text)" }}>
                <KindGlyph icon={g} size={18} />
              </button>
            ))}
          </div>
        </div>
        <div>
          <div className="req-flabel" style={{ marginBottom: 6 }}>Color</div>
          <div style={{ display: "flex", gap: 6 }}>
            {FORM_COLORS.map((c) => (
              <button key={c} onClick={() => setColor(c)} title={c} aria-label={`Color ${c}`}
                className="form-swatch" style={{ borderColor: color === c ? "var(--accent)" : "var(--border)" }}>
                <span style={{ width: 16, height: 16, borderRadius: 5, background: colorVar(c), display: "block" }} />
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Fields */}
      <div className="req-flabel" style={{ marginBottom: 6 }}>Fields</div>
      <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 10 }}>
        {fields.map((f, i) => (
          <div key={i} className="card" style={{ padding: 8, display: "flex", gap: 6, alignItems: "center", flexWrap: "wrap" }}>
            <div style={{ display: "flex", flexDirection: "column" }}>
              <button className="btn xs ghost" style={{ height: 16, padding: 0 }} onClick={() => move(i, -1)} disabled={i === 0} aria-label="Move up">▲</button>
              <button className="btn xs ghost" style={{ height: 16, padding: 0 }} onClick={() => move(i, 1)} disabled={i === fields.length - 1} aria-label="Move down">▼</button>
            </div>
            <input className="field" style={{ flex: "2 1 140px", height: 32 }} value={f.label_en}
              onChange={(e) => setField(i, { label_en: e.target.value })} placeholder="Label (e.g. Amount)" />
            <input className="field" style={{ flex: "2 1 120px", height: 32 }} value={f.label_th}
              onChange={(e) => setField(i, { label_th: e.target.value })} placeholder="ป้ายกำกับ (Thai)" />
            <select className="field" style={{ height: 32, flex: "1 1 100px" }} value={f.kind}
              onChange={(e) => setField(i, { kind: e.target.value })} aria-label="Field type">
              {FIELD_KINDS.map((k) => <option key={k} value={k}>{k}</option>)}
            </select>
            <label className="t-xs" style={{ display: "flex", gap: 4, alignItems: "center" }}>
              <input type="checkbox" checked={f.required} onChange={(e) => setField(i, { required: e.target.checked })} /> req.
            </label>
            <button className="btn xs ghost" onClick={() => removeField(i)} disabled={fields.length === 1} aria-label="Remove field">
              <Ico.x className="icon sm" />
            </button>
          </div>
        ))}
      </div>
      <button className="btn sm" onClick={addField}><Ico.plus className="icon sm" /> Add field</button>

      {/* Approval route */}
      <div style={{ marginTop: 16 }}>
        <div className="req-flabel" style={{ marginBottom: 6 }}>Approval route</div>
        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
          <select className="field" style={{ flex: "1 1 220px" }} value={templateId} onChange={(e) => setTemplateId(e.target.value)}>
            <option value="">— No route (choose approvers at submit) —</option>
            {templates.map((tp) => (
              <option key={tp.id} value={tp.id}>{tp.name} · {tp.steps.length} step{tp.steps.length === 1 ? "" : "s"}</option>
            ))}
          </select>
          <Link href="/settings/workflows" className="btn sm ghost"><Ico.layers className="icon sm" /> Design routes</Link>
        </div>
      </div>

      <div style={{ display: "flex", gap: 6, marginTop: 16 }}>
        <button className="btn primary sm" disabled={busy || !valid} onClick={submit}>{initial ? "Save changes" : "Create form"}</button>
        <button className="btn sm" disabled={busy} onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}
