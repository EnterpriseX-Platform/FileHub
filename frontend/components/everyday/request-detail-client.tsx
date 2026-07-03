"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { KindTile, KIND_META, StatusPill, money } from "@/components/everyday/request-bits";
import { WorkflowPanel } from "@/components/workflow-panel";
import { Ft } from "@/components/primitives";
import type { RequestDetail } from "@/lib/api";
import { fmtAgo, fmtBytes } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

/// Friendly request detail: the AI summary written for the approver, the
/// submitted form, the attached document, and the approval timeline (reused
/// WorkflowPanel — the same approve / reject / send-back the file view uses,
/// pointed at the request's anchor document).
export function RequestDetailClient({ req }: { req: RequestDetail }) {
  const { t, locale } = useI18n();
  const router = useRouter();
  const meta = KIND_META[req.kind];
  const entries = Object.entries(req.form_data || {}).filter(([, v]) => v != null && String(v).trim() !== "");

  return (
    <div className="page">
      <button className="req-back" onClick={() => router.push("/requests")}>
        <Ico.chevron className="icon sm" style={{ transform: "rotate(180deg)" }} /> {t("req.title")}
      </button>

      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 16, flexWrap: "wrap", marginTop: 6 }}>
        <div style={{ display: "flex", gap: 12, alignItems: "center", minWidth: 0 }}>
          <KindTile kind={req.kind} size={40} />
          <div style={{ minWidth: 0 }}>
            <div className="t-2xl t-semibold t-trunc">{req.title}</div>
            <div className="t-xs t-muted" style={{ marginTop: 4 }}>
              {meta.labelEn}{req.amount != null ? ` · ${money(req.amount)}` : ""} · {t("req.submittedBy", { name: req.requester_name })} · {fmtAgo(req.created_at)}
            </div>
          </div>
        </div>
        <div style={{ marginTop: 4 }}><StatusPill status={req.status} t={t} /></div>
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
