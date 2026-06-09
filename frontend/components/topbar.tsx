"use client";

import * as React from "react";

import { Ico } from "./icons";
import { NotificationsBell } from "./notifications-bell";
import { useSidebar } from "@/lib/sidebar-context";

export function TopBar({
  crumbs = [],
  actions,
  title,
}: {
  crumbs?: React.ReactNode[];
  actions?: React.ReactNode;
  title?: React.ReactNode;
}) {
  const { toggle } = useSidebar();
  return (
    <div className="topbar">
      <button
        type="button"
        className="btn icon ghost nav-toggle"
        onClick={toggle}
        aria-label="Open navigation"
        style={{ marginRight: 4 }}
      >
        <Ico.menu />
      </button>
      <div className="crumb">
        {crumbs.map((c, i) => (
          <React.Fragment key={i}>
            {i > 0 && <span className="crumb-sep"><Ico.chevron className="icon sm" /></span>}
            <span className={"crumb-item" + (i === crumbs.length - 1 && !title ? " active" : "")}>{c}</span>
          </React.Fragment>
        ))}
        {title && (
          <>
            {crumbs.length > 0 && <span className="crumb-sep"><Ico.chevron className="icon sm" /></span>}
            <span className="crumb-item active t-semibold">{title}</span>
          </>
        )}
      </div>
      <div style={{ flex: 1 }} />
      {/* Bell sits on every topbar by default; pages that pass a custom
          `actions` prop append next to it.  Previously each page rendered
          a decorative bell with no popover. */}
      <NotificationsBell tone="ghost" />
      {actions}
    </div>
  );
}
