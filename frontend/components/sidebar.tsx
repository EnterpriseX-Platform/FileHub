import * as React from "react";

import { GlobalSearch } from "./global-search";
import { Ico } from "./icons";
import { MyDriveLink } from "./my-drive-link";
import { SideLabel, SideRow } from "./primitives";
import { SavedViewsList } from "./saved-views-list";
import { UserMenu } from "./user-menu";
import { fmtCount } from "@/lib/format";
import type { Org, System, DashboardStats } from "@/lib/api";

export type NavKey = "dashboard" | "files" | "activity" | "views" | "share" | "archive" | "settings";

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
  const activeSystemId = systemActive ?? systems[0]?.id;

  const fileCountBySystem: Record<string, number> = {};
  for (const sys of stats?.connected_systems ?? []) {
    fileCountBySystem[sys.id] = sys.file_count;
  }

  return (
    <div className="side">
      <div className="ws">
        <span className="ws-logo" style={{ background: "#4f46e5" }}>F</span>
        <div className="ws-name">
          {stats?.workspace_display || "File Hub"}
          <div className="t-sm t-muted">{stats?.workspace_name || "acme.go.th"}</div>
        </div>
        <Ico.down />
      </div>

      <GlobalSearch />

      <div className="side-section">
        <SideRow href="/"          icon={<Ico.home />}     label="Dashboard" active={nav === "dashboard"} />
        <SideRow href="/files"     icon={<Ico.files />}    label="All files" active={nav === "files"} count={stats ? fmtCount(stats.total_files) : undefined} />
        <SideRow href="/activity"  icon={<Ico.activity />} label="Activity"  active={nav === "activity"} />
        <SideRow href="/views/new" icon={<Ico.views />}    label="Views"     active={nav === "views"} />
        <SideRow href="/share"     icon={<Ico.share />}    label="Shared"    active={nav === "share"} />
        <SideRow href="/archive"   icon={<Ico.archive />}  label="Archive"   active={nav === "archive"} />
        {/* My Drive — only renders for signed-in users; reads the personal
            drive id from the backend on mount. */}
        <MyDriveLink />
      </div>

      <div className="divider" style={{ margin: "4px 12px" }} />

      <div className="side-section">
        <SideLabel action={<Ico.plus className="icon sm" />}>Saved views</SideLabel>
        {/* Pulled live from /api/views (pinned rows).  Each click derives a
            `/files?field=value` URL from the first equality filter so the
            sidebar actually narrows the file table instead of being a dead
            label. */}
        <SavedViewsList />
      </div>

      <div className="side-section" style={{ flex: 1, overflow: "auto", minHeight: 0 }}>
        <SideLabel action={<span style={{ display: "flex", gap: 4 }}><Ico.filter className="icon sm" /><Ico.plus className="icon sm" /></span>}>
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
        <SideRow href="/trash"    icon={<Ico.trash />}    label="Trash" active={nav === "archive"} />
        <SideRow href="/settings" icon={<Ico.cog />}      label="Settings" active={nav === "settings"} />
        <UserMenu />
      </div>
    </div>
  );
}
