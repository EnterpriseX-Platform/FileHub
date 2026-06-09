import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFiles, safeOrgs, safeSearch, safeStats, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { canMutate } from "@/lib/roles";
import type { FileRow, System } from "@/lib/api";

import { ViewFilterBar } from "../view-filter-bar";
import { ViewTabs } from "../view-tabs";
import { fileMatchesFilters, readViewParams } from "../view-params";

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

type CalendarProps = { searchParams?: Promise<Record<string, string | string[] | undefined>> };

function ymd(iso: string): string {
  return iso.slice(0, 10);
}

/// Build a month grid (Sun-anchored). Returns a 6×7 array of either a Date or
/// null (for cells outside the active month).
function monthGrid(anchor: Date): Array<Array<Date | null>> {
  const year  = anchor.getFullYear();
  const month = anchor.getMonth();
  const first = new Date(Year(year, month, 1));
  const firstWeekday = first.getDay();
  const daysInMonth = new Date(Year(year, month + 1, 0)).getDate();

  const cells: Array<Array<Date | null>> = [];
  let day = 1 - firstWeekday;
  for (let row = 0; row < 6; row++) {
    const week: Array<Date | null> = [];
    for (let col = 0; col < 7; col++) {
      week.push(day >= 1 && day <= daysInMonth ? new Date(Year(year, month, day)) : null);
      day++;
    }
    cells.push(week);
  }
  return cells;
}

// Helper so we don't accidentally make a Y2K bug — pin year explicitly.
function Year(y: number, m: number, d: number): number {
  const t = new Date();
  t.setFullYear(y, m, d);
  t.setHours(0, 0, 0, 0);
  return t.getTime();
}

export default async function FilesCalendarPage({ searchParams }: CalendarProps) {
  const sp = (await searchParams) ?? {};
  const filters: Record<string, string> = {};
  for (const k of ["system_id", "org_id", "status", "project", "owner", "folder_id"] as const) {
    const v = sp[k];
    if (typeof v === "string" && v.length) filters[k] = v;
  }
  const searchTerm = typeof sp.q === "string" ? sp.q.trim() : "";
  const viewParams = readViewParams(sp);
  const monthParam = typeof sp.month === "string" ? sp.month : "";  // YYYY-MM
  const [year, month] = monthParam.match(/^(\d{4})-(\d{2})$/)
    ? [Number(monthParam.slice(0, 4)), Number(monthParam.slice(5, 7)) - 1]
    : [new Date().getFullYear(), new Date().getMonth()];

  const { cookieHeader, role } = await loadServerCtx();
  const [files, systems, stats] = await Promise.all([
    searchTerm ? safeSearch(searchTerm, cookieHeader) : safeFiles(filters, cookieHeader),
    safeSystems(cookieHeader),
    safeStats(cookieHeader),
  ]);
  const rows = files.filter((f) => fileMatchesFilters(f, filters));
  const sys = filters.system_id ? systems.find((s) => s.id === filters.system_id) : undefined;
  const orgs = (sys ?? systems[0]) ? await safeOrgs((sys ?? systems[0]).id, cookieHeader) : [];

  const grid = monthGrid(new Date(Year(year, month, 1)));
  const byDate: Record<string, FileRow[]> = {};
  for (const f of rows) {
    const k = ymd(f.modified_at);
    (byDate[k] ??= []).push(f);
  }

  const monthLabel = new Date(Year(year, month, 1)).toLocaleString("en-US", { month: "long", year: "numeric" });
  const prevMonth  = month === 0 ? `${year - 1}-12` : `${year}-${String(month).padStart(2, "0")}`;
  const nextMonth  = month === 11 ? `${year + 1}-01` : `${year}-${String(month + 2).padStart(2, "0")}`;
  const baseQs = (overrides: Record<string, string>) => {
    const merged: Record<string, string> = {};
    for (const [k, v] of Object.entries(viewParams)) if (v) merged[k] = v;
    Object.assign(merged, overrides);
    return `?${new URLSearchParams(merged).toString()}`;
  };

  return (
    <div className="scr">
      <Sidebar nav="files" systems={systems} stats={stats} orgs={orgs} systemActive={filters.system_id ?? systems[0]?.id} />
      <TopBar
        crumbs={sys ? ["Workspace", sys.name, "Calendar"] : ["Workspace", "Calendar"]}
        actions={canMutate(role) ? <a className="btn" href="/upload"><Ico.upload /> Upload</a> : undefined}
      />
      <div className="main">
        <div style={{ padding: "14px 24px 8px", borderBottom: "1px solid var(--border)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
            <ViewTabs params={viewParams} active="calendar" />
            <ViewFilterBar params={viewParams} base="/files/calendar" />
          </div>
        </div>

        <div style={{ padding: "16px 24px", borderBottom: "1px solid var(--border)", display: "flex", alignItems: "center", gap: 10 }}>
          <span className="t-2xl t-semibold">{monthLabel}</span>
          {sys && <Pill tone={sys.tone}><span className="dot" />{sys.name}</Pill>}
          <Pill>{rows.length} file{rows.length === 1 ? "" : "s"}</Pill>
          <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
            <a className="btn sm ghost icon" href={baseQs({ month: prevMonth })}><Ico.left className="icon sm" /></a>
            <a className="btn sm ghost" href={baseQs({ month: `${new Date().getFullYear()}-${String(new Date().getMonth() + 1).padStart(2, "0")}` })}>Today</a>
            <a className="btn sm ghost icon" href={baseQs({ month: nextMonth })}><Ico.chevron className="icon sm" /></a>
          </div>
        </div>

        <div style={{ flex: 1, overflow: "auto", padding: 24, background: "var(--bg-subtle)" }}>
          <div style={{ overflowX: "auto" }}>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(7, 1fr)", gap: 1, background: "var(--border)", minWidth: "700px" }}>
            {WEEKDAYS.map((d) => (
              <div key={d} style={{ background: "var(--bg)", padding: "8px 10px", fontSize: 11, fontWeight: 600, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.04em" }}>
                {d}
              </div>
            ))}
            {grid.flat().map((date, i) => {
              if (!date) {
                return <div key={i} style={{ background: "var(--bg-muted)", minHeight: 96 }} />;
              }
              const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
              const dayFiles = byDate[key] ?? [];
              const isToday = key === ymd(new Date().toISOString());
              return (
                <div key={i} style={{
                  background: "var(--bg)",
                  minHeight: 96, padding: "6px 8px",
                  display: "flex", flexDirection: "column", gap: 4,
                  outline: isToday ? "2px solid var(--accent)" : undefined,
                  outlineOffset: -1,
                }}>
                  <div className="t-xs t-tabular" style={{ color: isToday ? "var(--accent)" : "var(--text-muted)", fontWeight: 600 }}>
                    {date.getDate()}
                  </div>
                  {dayFiles.slice(0, 3).map((f) => (
                    <a key={f.id} href={`/files/${f.id}`} className="t-xs t-trunc"
                      style={{ color: "var(--text)", textDecoration: "none", padding: "2px 4px", borderRadius: 3, background: tonePillBg(systems, f) }}
                    >
                      {f.name}
                    </a>
                  ))}
                  {dayFiles.length > 3 && (
                    <div className="t-xs t-subtle">+ {dayFiles.length - 3} more</div>
                  )}
                </div>
              );
            })}
          </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function tonePillBg(systems: System[], f: FileRow): string {
  const sys = systems.find((s) => s.id === f.system_id);
  const tone = sys?.tone ?? "slate";
  // Soft tone background — picks up the per-system color, falls back to slate.
  return `var(--c-${tone}-soft, var(--bg-subtle))`;
}

