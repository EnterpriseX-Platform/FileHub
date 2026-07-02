"use client";

import Link from "next/link";
import * as React from "react";

import { FileAiPanel } from "@/app/files/[id]/ai-panel";
import { FilePreview } from "@/app/files/[id]/preview";
import { SignPanel } from "@/components/everyday/sign-panel";
import { StarButton } from "@/components/everyday/star-button";
import { Ico } from "@/components/icons";
import { WorkflowPanel } from "@/components/workflow-panel";
import { Av, Ft, Tag } from "@/components/primitives";
import { useAuth } from "@/lib/auth-context";
import type { FileRow } from "@/lib/api";
import { fmtAgo, fmtBytes, parseJsonArray } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { canMutate } from "@/lib/roles";

/// Friendly, preview-first file view for the everyday persona. Big preview on
/// the left, a plain info panel on the right (owner / area / when / size / type
/// / labels) plus the AI summary. Technical detail — status, project, version,
/// ETag, object key, raw permission rows — is intentionally absent; power users
/// reach it via "Open full details".
export function FileViewClient({ file, areaName }: { file: FileRow; areaName: string | null }) {
  const { t } = useI18n();
  const { user } = useAuth();
  const tags = parseJsonArray(file.tags);
  const download = `/filehub/api/files/${encodeURIComponent(file.id)}/download`;

  return (
    <div>
      <Link href="/my" className="eday-back">
        <Ico.chevron className="icon sm" style={{ transform: "rotate(180deg)" }} /> {t("eday.nav.myfiles")}
      </Link>

      <div className="eday-fileview-hd">
        <Ft type={file.file_type} size="lg" />
        <div style={{ flex: 1, minWidth: 0 }}>
          <h1 className="eday-h1 t-trunc" title={file.name}>{file.name}</h1>
          <div className="t-sm t-subtle">
            {fmtBytes(file.size_bytes)} · {fmtAgo(file.modified_at)}{areaName ? ` · ${areaName}` : ""}
          </div>
        </div>
        <a className="btn primary sm" href={download} download>
          <Ico.download className="icon sm" /> {t("eday.download")}
        </a>
        <StarButton fileId={file.id} />
        <Link className="btn sm" href="/ask">
          <span style={{ color: "var(--c-violet)", display: "inline-flex" }}><Ico.sparkle className="icon sm" /></span> {t("eday.ask")}
        </Link>
      </div>

      <div className="eday-fileview">
        <div className="eday-preview">
          <FilePreview fileId={file.id} fileType={file.file_type} fileName={file.name} />
        </div>
        <aside className="eday-fileinfo">
          <FileAiPanel fileId={file.id} />
          <div className="divider" style={{ margin: "14px 0" }} />
          <SignPanel fileId={file.id} canRequest={canMutate(user?.role ?? null)} />
          <div className="divider" style={{ margin: "14px 0" }} />
          <WorkflowPanel fileId={file.id} />
          <div className="divider" style={{ margin: "14px 0" }} />
          <Meta label={t("eday.owner")}>
            <Av name={file.owner} tone="slate" /> <span className="t-sm t-trunc">{file.owner}</span>
          </Meta>
          {areaName && <Meta label={t("eday.area")}><span className="t-sm">{areaName}</span></Meta>}
          <Meta label={t("eday.modified")}><span className="t-sm">{fmtAgo(file.modified_at)}</span></Meta>
          <Meta label={t("eday.size")}><span className="t-sm">{fmtBytes(file.size_bytes)}</span></Meta>
          <Meta label={t("eday.type")}><span className="t-sm" style={{ textTransform: "uppercase" }}>{file.file_type}</span></Meta>
          {tags.length > 0 && (
            <Meta label={t("eday.labels")}>
              <span style={{ display: "flex", flexWrap: "wrap", gap: 4 }}>{tags.map((tg) => <Tag key={tg}>{tg}</Tag>)}</span>
            </Meta>
          )}
          {canMutate(user?.role ?? null) && (
            <Link href={`/files/${encodeURIComponent(file.id)}`} className="t-xs t-subtle eday-fulllink">
              {t("eday.fullDetails")} <Ico.chevron className="icon sm" style={{ verticalAlign: "-2px" }} />
            </Link>
          )}
        </aside>
      </div>
    </div>
  );
}

function Meta({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="eday-meta">
      <div className="t-xs t-subtle">{label}</div>
      <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>{children}</div>
    </div>
  );
}
