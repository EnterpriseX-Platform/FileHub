"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { KindTile, StatusPill, money } from "@/components/everyday/request-bits";
import { WorkflowPanel } from "@/components/workflow-panel";
import { Ft } from "@/components/primitives";
import { useAuth } from "@/lib/auth-context";
import type { RequestDetail } from "@/lib/api";
import { fmtAgo, fmtBytes } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

/// Friendly request detail: the AI summary written for the approver, the
/// submitted form, the attached document, and the approval timeline (reused
/// WorkflowPanel — the same approve / reject / send-back the file view uses,
/// pointed at the request's anchor document).
export function RequestDetailClient({ req }: { req: RequestDetail }) {
  const { t } = useI18n();
  const { user } = useAuth();
  const router = useRouter();
  const [withdrawing, setWithdrawing] = React.useState(false);
  const [confirm, setConfirm] = React.useState(false);
  const entries = Object.entries(req.form_data || {}).filter(([, v]) => v != null && String(v).trim() !== "");

  // The requester may withdraw while the request is still open (not decided/withdrawn).
  const canWithdraw = !!user && req.requester_id === user.id && (req.status === "in_review" || req.status === "submitted");

  async function withdraw() {
    setWithdrawing(true);
    try {
      const r = await fetch(`/filehub/api/requests/${encodeURIComponent(req.id)}/cancel`, { method: "POST", credentials: "include" });
      if (r.ok) router.refresh();
    } finally {
      setWithdrawing(false);
      setConfirm(false);
    }
  }

  return (
    <div className="page">
      <button className="req-back" onClick={() => router.push("/requests")}>
        <Ico.chevron className="icon sm" style={{ transform: "rotate(180deg)" }} /> {t("req.title")}
      </button>

      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 16, flexWrap: "wrap", marginTop: 6 }}>
        <div style={{ display: "flex", gap: 12, alignItems: "center", minWidth: 0 }}>
          <KindTile icon={req.icon} color={req.color} size={40} />
          <div style={{ minWidth: 0 }}>
            <div className="t-2xl t-semibold t-trunc">{req.title}</div>
            <div className="t-xs t-muted" style={{ marginTop: 4 }}>
              {req.kind_label}{req.amount != null ? ` · ${money(req.amount)}` : ""} · {t("req.submittedBy", { name: req.requester_name })} · {fmtAgo(req.created_at)}
            </div>
          </div>
        </div>
        <div style={{ marginTop: 4, display: "flex", alignItems: "center", gap: 10 }}>
          <StatusPill status={req.status} t={t} />
          {canWithdraw && !confirm && (
            <button className="btn ghost xs" onClick={() => setConfirm(true)}>{t("req.withdraw")}</button>
          )}
          {canWithdraw && confirm && (
            <span style={{ display: "inline-flex", gap: 6, alignItems: "center" }}>
              <button className="btn danger xs" disabled={withdrawing} onClick={withdraw}>{withdrawing ? "…" : t("req.withdrawConfirm")}</button>
              <button className="btn ghost xs" disabled={withdrawing} onClick={() => setConfirm(false)}>{t("req.cancel")}</button>
            </span>
          )}
        </div>
      </div>

      <div className="req-detail-grid">
        <div>
          {req.ai_summary && (
            <div className="req-aisum" style={{ marginBottom: 18 }}>
              <div className="req-aisum-h"><Ico.sparkle className="icon sm" /> {t("req.forApprover")}</div>
              <div className="t-sm" style={{ lineHeight: 1.6 }}>{req.ai_summary}</div>
            </div>
          )}

          <div className="card" style={{ overflow: "hidden" }}>
            <div className="req-cardhd">{t("req.details")}</div>
            <div className="req-formgrid">
              {entries.length === 0 ? (
                <div className="req-fld full"><div className="t-sm t-subtle">—</div></div>
              ) : entries.map(([k, v]) => (
                <div key={k} className={"req-fld" + (String(v).length > 40 ? " full" : "")}>
                  <div className="req-flabel">{k.replace(/_/g, " ")}</div>
                  <div className="t-sm t-medium" style={{ marginTop: 2 }}>{String(v)}</div>
                </div>
              ))}
            </div>
            {req.file && (
              <div style={{ padding: "14px 16px", borderTop: "1px solid var(--border)" }}>
                <div className="req-flabel" style={{ marginBottom: 7 }}>{t("req.attached")}</div>
                <Link href={`/f/${encodeURIComponent(req.file.id)}`} className="req-attached" style={{ textDecoration: "none", color: "inherit" }}>
                  <Ft type={req.file.file_type} size="lg" />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div className="t-sm t-medium t-trunc">{req.file.name}</div>
                    <div className="t-xs t-subtle">{fmtBytes(req.file.size_bytes)} · {req.file.file_type.toUpperCase()}</div>
                  </div>
                  <span className="t-xs t-subtle">{t("eday.reviewGo")}</span>
                </Link>
              </div>
            )}
          </div>

          <ActivityTrail req={req} t={t} />
        </div>

        <div>
          {req.file
            ? <WorkflowPanel fileId={req.file.id} hideStart />
            : <div className="card pad t-sm t-subtle">No approval route.</div>}
        </div>
      </div>
    </div>
  );
}

/// A chronological narrative of the request's life — submitted, each decision,
/// and who's up next. Derived from the request + its workflow steps (no extra
/// fetch); complements the WorkflowPanel's status view with a plain history.
function ActivityTrail({ req, t }: {
  req: RequestDetail;
  t: (k: string, p?: Record<string, string | number>) => string;
}) {
  type Ev = { when: string; who: string; text: string; note?: string; tone: string };
  const events: Ev[] = [
    { when: req.created_at, who: req.requester_name, text: t("req.evSubmitted"), tone: "slate" },
  ];
  for (const s of req.steps) {
    if ((s.decision === "approved" || s.decision === "rejected") && s.decided_at) {
      events.push({
        when: s.decided_at,
        who: s.reviewer_name ?? "Reviewer",
        text: s.decision === "approved" ? t("req.evApproved", { step: s.name ?? "" }) : t("req.evRejected", { step: s.name ?? "" }),
        note: s.note ?? undefined,
        tone: s.decision === "approved" ? "emerald" : "rose",
      });
    }
  }
  events.sort((a, b) => a.when.localeCompare(b.when));
  const pending = req.steps.find((s) => s.decision === "pending");

  return (
    <div className="card" style={{ marginTop: 18, overflow: "hidden" }}>
      <div className="req-cardhd">{t("req.activity")}</div>
      <div style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 14 }}>
        {events.map((e, i) => (
          <div key={i} style={{ display: "flex", gap: 11 }}>
            <span style={{ width: 8, height: 8, borderRadius: 999, marginTop: 6, flexShrink: 0, background: `var(--c-${e.tone})` }} />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="t-sm"><b className="t-semibold">{e.who}</b> {e.text}</div>
              {e.note && <div className="t-xs t-muted" style={{ marginTop: 2 }}>&ldquo;{e.note}&rdquo;</div>}
            </div>
            <span className="t-xs t-subtle" style={{ flexShrink: 0 }}>{fmtAgo(e.when)}</span>
          </div>
        ))}
        {pending && (
          <div style={{ display: "flex", gap: 11 }}>
            <span style={{ width: 8, height: 8, borderRadius: 999, marginTop: 6, flexShrink: 0, border: "2px solid var(--c-amber)" }} />
            <div className="t-sm t-muted" style={{ flex: 1 }}>{t("req.evWaiting", { name: pending.reviewer_name ?? "a reviewer" })}</div>
          </div>
        )}
      </div>
    </div>
  );
}
