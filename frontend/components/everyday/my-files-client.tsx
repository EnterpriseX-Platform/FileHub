"use client";

import Link from "next/link";
import * as React from "react";

import { FileListRow, FileTile } from "@/components/everyday/pieces";
import { Ico } from "@/components/icons";
import { Empty } from "@/components/primitives";
import type { FileRow } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

type Area = { id: string; name: string; tone: string };

/// Plain file browser for the everyday persona. Area filter chips + an in-page
/// filter box + grid/list toggle (two options, not five), and a friendly empty
/// state. No status / project / tag / version columns.
export function MyFilesClient({
  files,
  titleKey,
  title,
  emptyKey = "eday.noFiles",
  canUpload,
  areas = [],
}: {
  files: FileRow[];
  titleKey?: string;
  title?: string;
  emptyKey?: string;
  canUpload: boolean;
  areas?: Area[];
}) {
  const { t } = useI18n();
  const [view, setView] = React.useState<"grid" | "list">("grid");
  const [q, setQ] = React.useState("");
  const [area, setArea] = React.useState<string>("all");

  const heading = titleKey ? t(titleKey) : (title ?? t("eday.nav.myfiles"));
  const term = q.trim().toLowerCase();
  const areaName = React.useMemo(() => {
    const m = new Map(areas.map((a) => [a.id, a.name]));
    return (systemId: string) => m.get(systemId);
  }, [areas]);
  // Only offer chips for areas that actually contain some of these files.
  const chipAreas = React.useMemo(
    () => areas.filter((a) => files.some((f) => f.system_id === a.id)),
    [areas, files],
  );
  const shown = files.filter((f) =>
    (area === "all" || f.system_id === area) &&
    (!term || f.name.toLowerCase().includes(term)));

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

      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", margin: "18px 0 4px" }}>
        {chipAreas.length > 1 && (
          <>
            <button className={"eday-fchip" + (area === "all" ? " on" : "")} onClick={() => setArea("all")}>
              {t("eday.all")}
            </button>
            {chipAreas.map((a) => (
              <button key={a.id} className={"eday-fchip" + (area === a.id ? " on" : "")} onClick={() => setArea(a.id)}>
                {a.name}
              </button>
            ))}
          </>
        )}
        <div className="field" style={{ maxWidth: 260, height: 36, borderRadius: 999, marginLeft: "auto" }}>
          <Ico.search />
          <input placeholder={t("search.button") + "…"} value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
      </div>

      {shown.length === 0 ? (
        <Empty
          icon={term ? <Ico.search className="icon" /> : <Ico.files className="icon" />}
          title={term ? t("cmd.noMatch") : t(emptyKey)}
          hint={!term && canUpload ? t("eday.subFirst") : undefined}
          action={!term && canUpload ? (
            <Link href="/upload" className="btn primary sm"><Ico.upload className="icon sm" /> {t("nav.upload")}</Link>
          ) : undefined}
        />
      ) : view === "grid" ? (
        <div className="eday-cards" style={{ marginTop: 16 }}>
          {shown.map((f) => <FileTile key={f.id} file={f} area={areaName(f.system_id)} />)}
        </div>
      ) : (
        <div style={{ marginTop: 12, display: "flex", flexDirection: "column", gap: 1 }}>
          {shown.map((f) => <FileListRow key={f.id} file={f} />)}
        </div>
      )}
    </div>
  );
}
