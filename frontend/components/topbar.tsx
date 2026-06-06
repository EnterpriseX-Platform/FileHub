import * as React from "react";

import { Ico } from "./icons";
import { NotificationsBell } from "./notifications-bell";

export function TopBar({
  crumbs = [],
  actions,
  title,
}: {
  crumbs?: React.ReactNode[];
  actions?: React.ReactNode;
  title?: React.ReactNode;
}) {
  return (
    <div className="topbar">
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
