"use client";

import Link from "next/link";
import * as React from "react";

import { GlobalSearch } from "./global-search";
import { Ico } from "./icons";
import { MyDriveLink } from "./my-drive-link";
import { SideRow } from "./primitives";
import { UserMenu } from "./user-menu";
import { fmtCount } from "@/lib/format";
import { useSidebar } from "@/lib/sidebar-context";
import type { Org, System, DashboardStats } from "@/lib/api";

export type NavKey = "dashboard" | "files" | "activity" | "views" | "share" | "archive" | "trash" | "settings";

/// Global navigation.  Buckets / folders / tag folders live in the Files
/// explorer tree (/explorer) — keeping them here too made two competing trees.
/// `systems` / `orgs` / `systemActive` / `orgActive` are still accepted so
/// existing pages compile, but are no longer rendered.
export function Sidebar({
  nav = "files",
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

  return (
    <>
    <div className={"side" + (open ? " open" : "")}>
      <div className="ws">
        <span className="ws-logo" style={{ background: "var(--accent)" }}>F</span>
        <div className="ws-name">
          {stats?.workspace_display || "File Hub"}
          <div className="t-sm t-muted">{stats?.workspace_name || ""}</div>
        </div>
      </div>

      <GlobalSearch />

      <div style={{ padding: "4px 12px 8px" }}>
        <Link href="/upload" className="btn primary" style={{ width: "100%", justifyContent: "center" }}>
          <Ico.upload className="icon sm" /> Upload files
        </Link>
      </div>

      <div className="side-section" style={{ flex: 1 }}>
        <SideRow href="/"          icon={<Ico.home />}     label="Overview" active={nav === "dashboard"} />
        <SideRow href="/explorer"  icon={<Ico.files />}    label="Files" active={nav === "files"}
                 count={stats ? fmtCount(stats.total_files) : undefined} />
        <SideRow href="/activity"  icon={<Ico.activity />} label="Activity" active={nav === "activity"} />
        <SideRow href="/share"     icon={<Ico.share />}    label="Shared links" active={nav === "share"} />
        <SideRow href="/archive"   icon={<Ico.archive />}  label="Archive" active={nav === "archive"} />
        {/* My Drive — only renders for signed-in users; reads the personal
            drive id from the backend on mount. */}
        <MyDriveLink />
      </div>

      <div style={{ borderTop: "1px solid var(--border)", padding: "8px 8px" }}>
        <SideRow href="/trash"    icon={<Ico.trash />}    label="Trash" active={nav === "trash"} />
        <SideRow href="/settings" icon={<Ico.cog />}      label="Settings" active={nav === "settings"} />
        <UserMenu />
      </div>
    </div>
    {open && <div className="side-backdrop open" onClick={() => setOpen(false)} aria-hidden="true" />}
    </>
  );
}
