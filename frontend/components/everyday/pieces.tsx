"use client";

import Link from "next/link";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Av, Ft } from "@/components/primitives";
import type { FileRow } from "@/lib/api";
import { fmtAgo, fmtBytes } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

/// Shared everyday building blocks. Deliberately plain: a file is shown by its
/// icon, name, and a friendly "who · when" line — no status/project/tag/version
/// columns. (Phase 2 swaps the file link for a friendly /f/[id] view; for now
/// it opens the existing detail page.)
function fileHref(id: string) {
  return `/files/${encodeURIComponent(id)}`;
}

export function EdaySection({
  titleKey,
  viewAll,
  href,
  children,
}: {
  titleKey: string;
  viewAll?: boolean;
  href?: string;
  children: React.ReactNode;
}) {
  const { t } = useI18n();
  return (
    <section>
      <div className="eday-section-hd">
        <h2>{t(titleKey)}</h2>
        {viewAll && href && (
          <Link href={href} className="t-sm" style={{ marginLeft: "auto", color: "var(--accent-text)", textDecoration: "none" }}>
            {t("eday.viewAll")} <Ico.chevron className="icon sm" style={{ verticalAlign: "-2px" }} />
          </Link>
        )}
      </div>
      {children}
    </section>
  );
}

export function FileTile({ file }: { file: FileRow }) {
  return (
    <Link href={fileHref(file.id)} className="eday-filecard">
      <Ft type={file.file_type} size="lg" />
      <div>
        <div className="t-sm t-semibold t-trunc" title={file.name}>{file.name}</div>
        <div className="t-xs t-subtle" style={{ marginTop: 3 }}>
          {fmtBytes(file.size_bytes)} · {fmtAgo(file.modified_at)}
        </div>
      </div>
    </Link>
  );
}

export function ReviewRow({ file }: { file: FileRow }) {
  const { t } = useI18n();
  return (
    <Link href={fileHref(file.id)} className="eday-filerow">
      <Ft type={file.file_type} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="t-sm t-medium t-trunc">{file.name}</div>
        <div className="t-xs t-subtle t-trunc">{file.owner} · {fmtAgo(file.modified_at)}</div>
      </div>
      <span className="btn xs primary" style={{ pointerEvents: "none" }}>{t("eday.review")}</span>
    </Link>
  );
}

export function FileListRow({ file }: { file: FileRow }) {
  return (
    <Link href={fileHref(file.id)} className="eday-filerow">
      <Ft type={file.file_type} />
      <span className="t-sm t-medium t-trunc" style={{ flex: 1, minWidth: 0 }}>{file.name}</span>
      <span className="t-xs t-subtle" style={{ width: 90, textAlign: "right", flexShrink: 0 }}>{fmtBytes(file.size_bytes)}</span>
      <span className="t-xs t-subtle only-desktop" style={{ width: 110, textAlign: "right", flexShrink: 0 }}>{fmtAgo(file.modified_at)}</span>
    </Link>
  );
}

export function OwnerBadge({ name, tone }: { name: string; tone: string }) {
  return <Av name={name} tone={(tone as never) || "slate"} />;
}
