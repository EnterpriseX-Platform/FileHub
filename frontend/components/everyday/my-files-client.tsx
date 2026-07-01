"use client";

import Link from "next/link";
import * as React from "react";

import { FileListRow, FileTile } from "@/components/everyday/pieces";
import { Ico } from "@/components/icons";
import type { FileRow } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

/// Plain file browser for the everyday persona. Grid/list toggle (two options,
/// not five), an in-page filter box, and a friendly empty state. No status /
/// project / tag / version columns.
export function MyFilesClient({
  files,
  titleKey,
  title,
  canUpload,
}: {
  files: FileRow[];
  titleKey?: string;
  title?: string;
  canUpload: boolean;
}) {
  const { t } = useI18n();
  const [view, setView] = React.useState<"grid" | "list">("grid");
  const [q, setQ] = React.useState("");

  const heading = titleKey ? t(titleKey) : (title ?? t("eday.nav.myfiles"));
  const term = q.trim().toLowerCase();
  const shown = term ? files.filter((f) => f.name.toLowerCase().includes(term)) : files;

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <h1 className="eday-h1">{heading}</h1>
        <span className="t-sm t-subtle">{files.length}</span>
        <div style={{ flex: 1 }} />
        {canUpload && (
          <Link href="/upload" className="btn sm primary"><Ico.upload className="icon sm" /> {t("nav.upload")}</Link>
        )}
        <div className="eday-viewtoggle">
          <button className={"btn icon sm ghost" + (view === "grid" ? " active" : "")} aria-label="Grid view" aria-pressed={view === "grid"} onClick={() => setView("grid")}><Ico.gallery className="icon sm" /></button>
          <button className={"btn icon sm ghost" + (view === "list" ? " active" : "")} aria-label="List view" aria-pressed={view === "list"} onClick={() => setView("list")}><Ico.table className="icon sm" /></button>
        </div>
      </div>

      <div className="field" style={{ maxWidth: 340, height: 34, marginTop: 14 }}>
        <Ico.search />
        <input placeholder={t("search.button") + "…"} value={q} onChange={(e) => setQ(e.target.value)} />
      </div>

      {shown.length === 0 ? (
        <div className="t-sm t-subtle" style={{ padding: "28px 0", textAlign: "center" }}>{t("eday.noFiles")}</div>
      ) : view === "grid" ? (
        <div className="eday-cards" style={{ marginTop: 16 }}>
          {shown.map((f) => <FileTile key={f.id} file={f} />)}
        </div>
      ) : (
        <div style={{ marginTop: 12, display: "flex", flexDirection: "column", gap: 1 }}>
          {shown.map((f) => <FileListRow key={f.id} file={f} />)}
        </div>
      )}
    </div>
  );
}
