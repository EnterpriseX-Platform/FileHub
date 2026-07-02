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
/// columns. Files open the friendly /f/[id] view, not the technical detail page.
function fileHref(id: string) {
  return `/f/${encodeURIComponent(id)}`;
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

// Image types get a real server thumbnail (GET /api/files/:id/thumb); anything
// else shows a large type badge on the tinted tile. onError falls back to the
// badge, so a missing/failed thumbnail never leaves a broken image.
const THUMB_TYPES = new Set(["img", "jpg", "jpeg", "png", "gif", "webp"]);

export function FileTile({ file }: { file: FileRow }) {
  const [imgOk, setImgOk] = React.useState(true);
  const showImg = THUMB_TYPES.has(file.file_type.toLowerCase()) && imgOk;
  return (
    <Link href={fileHref(file.id)} className="eday-filecard">
      <div className="thumb">
        {showImg ? (
          /* API-served thumbnail behind session auth — next/image can't
             optimize it, so the plain element is intentional. */
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={`/filehub/api/files/${encodeURIComponent(file.id)}/thumb`}
            alt=""
            loading="lazy"
            onError={() => setImgOk(false)}
          />
        ) : (
          <Ft type={file.file_type} size="xl" />
        )}
      </div>
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
      <Ft type={file.file_type} size="lg" />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="t-md t-semibold t-trunc">{file.name}</div>
        <div className="t-xs t-subtle t-trunc">
          {t("eday.sentBy", { name: file.owner })} · {fmtAgo(file.modified_at)}
        </div>
      </div>
      <span className="eday-act">{t("eday.reviewGo")}</span>
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
