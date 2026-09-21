import Link from "next/link";

import { Ico } from "@/components/icons";

import { buildViewHref, type ViewParams } from "./view-params";

// Layout tabs shared by every /files view. Each link carries the *full* current
// view params (filters, search, sort, group, columns) so switching layout keeps
// your context instead of resetting to "all files".
const TABS: Array<{ key: string; base: string; label: string; icon: React.ReactNode }> = [
  { key: "table",    base: "/files",          label: "ตาราง",    icon: <Ico.table className="icon sm" /> },
  { key: "board",    base: "/files/board",    label: "บอร์ด",    icon: <Ico.board className="icon sm" /> },
  { key: "gallery",  base: "/files/gallery",  label: "แกลเลอรี",  icon: <Ico.gallery className="icon sm" /> },
  { key: "calendar", base: "/files/calendar", label: "ปฏิทิน", icon: <Ico.cal className="icon sm" /> },
  { key: "timeline", base: "/files/timeline", label: "ไทม์ไลน์", icon: <Ico.timeline className="icon sm" /> },
];

export function ViewTabs({ params, active }: { params: ViewParams; active: string }) {
  return (
    <div className="seg">
      {TABS.map((t) => (
        <Link key={t.key} href={buildViewHref(t.base, params)} className={"seg-item" + (active === t.key ? " active" : "")}>
          {t.icon} {t.label}
        </Link>
      ))}
    </div>
  );
}
