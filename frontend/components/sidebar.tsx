"use client";

import Link from "next/link";
import * as React from "react";

import { GlobalSearch } from "./global-search";
import { Ico } from "./icons";
import { MyDriveLink } from "./my-drive-link";
import { SideLabel, SideRow } from "./primitives";
import { SavedViewsList } from "./saved-views-list";
import { UserMenu } from "./user-menu";
import { fmtCount } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { useSidebar } from "@/lib/sidebar-context";
import type { Org, System, DashboardStats } from "@/lib/api";

export type NavKey = "dashboard" | "ask" | "files" | "search" | "activity" | "reports" | "views" | "share" | "archive" | "trash" | "settings";

/// Sidebar is a pure synchronous component so it can render correctly inside
/// both server and client pages. Data is supplied entirely via props; pages
/// that want to show org rollups under the active system pass `orgs` in.
export function Sidebar({
  nav = "files",
  systemActive,
  orgActive,
  systems = [],
  orgs = [],
  stats,
}: {
  nav?: NavKey;
  systemActive?: string;
  orgActive?: string;
  systems?: System[];
  orgs?: Org[];
  stats?: DashboardStats | null;
}) {
  const { open, setOpen } = useSidebar();
  const { t } = useI18n();
  const activeSystemId = systemActive ?? systems[0]?.id;

  // Secondary destinations live under a collapsible "More" so the primary rail
  // stays short. Auto-expand when the user is already on one of them.
  const inMore = nav === "activity" || nav === "reports" || nav === "archive";
  const [moreOpen, setMoreOpen] = React.useState(inMore);

  const fileCountBySystem: Record<string, number> = {};
  for (const sys of stats?.connected_systems ?? []) {
    fileCountBySystem[sys.id] = sys.file_count;
  }

  return (
    <>
    <div className={"side" + (open ? " open" : "")}>
      <div className="ws">
        <span className="ws-logo" style={{ background: "var(--grad)" }}>F</span>
        <div className="ws-name">
          {stats?.workspace_display || "File Hub"}
          <div className="t-sm t-muted">{stats?.workspace_name || "acme.go.th"}</div>
        </div>
        <Ico.down />
      </div>

      <GlobalSearch />

      <div className="side-section">
        <SideRow href="/"          icon={<Ico.home />}     label={t("nav.dashboard")} active={nav === "dashboard"} />
        <SideRow href="/ask"       icon={<Ico.sparkle />}  label={t("nav.ask")} active={nav === "ask"} />
        <SideRow href="/files"     icon={<Ico.files />}    label={t("nav.files")} active={nav === "files"} count={stats ? fmtCount(stats.total_files) : undefined} />
        <SideRow href="/share"     icon={<Ico.share />}    label={t("nav.shared")}    active={nav === "share"} />
        {/* My Drive — only renders for signed-in users; reads the personal
            drive id from the backend on mount. */}
        <MyDriveLink />

        {/* Secondary destinations, collapsed by default to keep the rail short. */}
        <button
          type="button"
          className={"side-row" + (moreOpen ? " active" : "")}
          aria-expanded={moreOpen}
          onClick={() => setMoreOpen((o) => !o)}
          style={{ width: "100%", background: "none", border: 0, cursor: "pointer", textAlign: "left", font: "inherit" }}
        >
          <Ico.more />
          <span>{t("nav.more")}</span>
          <span className="count"><Ico.chevron className="icon sm" style={{ transform: moreOpen ? "rotate(90deg)" : "none" }} /></span>
        </button>
        {moreOpen && (
          <>
            <SideRow indent={1} href="/activity"  icon={<Ico.activity />} label={t("nav.activity")} active={nav === "activity"} />
            <SideRow indent={1} href="/reports"   icon={<Ico.history />}  label={t("nav.reports")}  active={nav === "reports"} />
            <SideRow indent={1} href="/archive"   icon={<Ico.archive />}  label={t("nav.archive")}  active={nav === "archive"} />
          </>
        )}
      </div>

      <div className="divider" style={{ margin: "4px 12px" }} />

      <div className="side-section">
        <SideLabel action={<Link href="/views/new" title="Create a saved view" aria-label="Create a saved view" style={{ display: "inline-flex", color: "inherit" }}><Ico.plus className="icon sm" /></Link>}>
          <Link href="/views" title="Manage all saved views" style={{ color: "inherit", textDecoration: "none" }}>{t("nav.savedViews")}</Link>
        </SideLabel>
        {/* Pulled live from /api/views (pinned rows).  Each click derives a
            `/files?field=value` URL from the first equality filter so the
            sidebar actually narrows the file table instead of being a dead
            label. */}
        <SavedViewsList />
      </div>

      <div className="side-section" style={{ flex: 1, overflow: "auto", minHeight: 0 }}>
        <SideLabel>
          Systems · {systems.length}
        </SideLabel>

        {systems.length === 0 && (
          <div style={{ padding: "8px 10px", fontSize: 12, color: "var(--text-subtle)" }}>
            No systems yet.
          </div>
        )}

        {systems.map((sys) => {
          const isActive = sys.id === activeSystemId;
          const count = fileCountBySystem[sys.id];
          return (
            <React.Fragment key={sys.id}>
              <SideRow
                icon={isActive ? <Ico.down className="icon sm" /> : <Ico.chevron className="icon sm" />}
                label={sys.name}
                active={isActive}
                count={count != null ? count : undefined}
                href={`/files?system_id=${sys.id}`}
              >
                <span className={"pill " + sys.tone + " sm"} style={{ marginLeft: 6, height: 14, padding: "0 5px" }}>
                  <span className="dot" />
                </span>
              </SideRow>
              {isActive && orgs.slice(0, 5).map((o) => (
                <SideRow
                  key={o.id}
                  indent={1}
                  icon={<Ico.bucket className="icon sm" />}
                  label={o.name}
                  active={orgActive === o.id}
                  href={`/files?system_id=${o.system_id}&org_id=${o.id}`}
                />
              ))}
              {isActive && orgs.length > 5 && (
                <a href="/orgs" style={{ paddingLeft: 28, fontSize: 11, color: "var(--text-subtle)", padding: "4px 10px 4px 28px", display: "block" }}>
                  + {orgs.length - 5} more org{orgs.length - 5 === 1 ? "" : "s"}
                </a>
              )}
            </React.Fragment>
          );
        })}
      </div>

      <div style={{ borderTop: "1px solid var(--border)", padding: "8px 8px" }}>
        <SideRow href="/trash"    icon={<Ico.trash />}    label={t("nav.trash")} active={nav === "trash"} />
        <SideRow href="/settings" icon={<Ico.cog />}      label={t("nav.settings")} active={nav === "settings"} />
        <UserMenu />
      </div>
    </div>
    {open && <div className="side-backdrop open" onClick={() => setOpen(false)} aria-hidden="true" />}
    </>
  );
}
