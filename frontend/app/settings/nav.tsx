import Link from "next/link";

import { Ico } from "@/components/icons";

export type SettingsTab = "general" | "members" | "roles" | "forms" | "workflows" | "audit";

const TABS: Array<[SettingsTab, string, React.ComponentType<{ className?: string }>]> = [
  ["general",   "General",             Ico.cog],
  ["members",   "Members",             Ico.users],
  ["roles",     "Roles & permissions", Ico.shield],
  ["forms",     "Request forms",       Ico.file],
  ["workflows", "Workflow templates",  Ico.layers],
  ["audit",     "Audit log",           Ico.history],
];

/// Shared left-rail nav for every /settings/* page. Only pages that exist are
/// listed — unbuilt areas live in TODO.md, not as dead nav entries.
export function SettingsNav({ active }: { active: SettingsTab }) {
  return (
    <div style={{ borderRight: "1px solid var(--border)", padding: "16px 12px", overflow: "auto", background: "var(--bg-subtle)" }}>
      <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", padding: "4px 10px 8px" }}>
        Workspace
      </div>
      {TABS.map(([id, label, Icon]) => (
        <Link key={id} href={`/settings${id === "general" ? "" : "/" + id}`} className={"side-row" + (active === id ? " active" : "")}>
          <Icon className="icon" />
          <span style={{ flex: 1 }}>{label}</span>
        </Link>
      ))}
    </div>
  );
}
