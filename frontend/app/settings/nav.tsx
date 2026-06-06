import Link from "next/link";

import { Ico } from "@/components/icons";

export type SettingsTab =
  | "general" | "members" | "roles" | "orgs" | "storage" | "fields"
  | "automation" | "integrations" | "audit" | "billing";

const TABS: Array<[SettingsTab, string, React.ComponentType<{ className?: string }>, boolean]> = [
  // [id, label, icon, implemented]
  ["general",     "General",              Ico.cog,     true],
  ["members",     "Members",              Ico.users,   true],
  ["roles",       "Roles & permissions",  Ico.shield,  true],
  ["audit",       "Audit log",            Ico.history, true],
  ["orgs",        "Orgs & systems",       Ico.layers,  false],
  ["storage",     "Storage backends",     Ico.database,false],
  ["fields",      "Custom fields",        Ico.tag,     false],
  ["automation",  "Automation rules",     Ico.bolt,    false],
  ["integrations","Integrations",         Ico.globe,   false],
  ["billing",     "Billing",              Ico.bolt,    false],
];

/// Shared left-rail nav for every /settings/* page.  Renders one column with
/// the eight settings tabs; the ones we haven't built yet are dimmed and
/// hover-tipped "Coming soon" so users can tell wired vs not.
export function SettingsNav({ active }: { active: SettingsTab }) {
  return (
    <div style={{ borderRight: "1px solid var(--border)", padding: "16px 12px", overflow: "auto", background: "var(--bg-subtle)" }}>
      <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", padding: "4px 10px 8px" }}>
        Workspace
      </div>
      {TABS.map(([id, label, Icon, implemented]) => {
        const cls = "side-row" + (active === id ? " active" : "");
        if (!implemented) {
          return (
            <div key={id} className={cls} style={{ opacity: 0.4, cursor: "not-allowed" }} title="Coming soon">
              <Icon className="icon" />
              <span style={{ flex: 1 }}>{label}</span>
              <span className="t-xs t-subtle">soon</span>
            </div>
          );
        }
        return (
          <Link key={id} href={`/settings${id === "general" ? "" : "/" + id}`} className={cls}>
            <Icon className="icon" />
            <span style={{ flex: 1 }}>{label}</span>
          </Link>
        );
      })}
    </div>
  );
}
