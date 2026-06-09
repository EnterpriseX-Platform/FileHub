import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeMembers, safeSystems } from "@/lib/api";

import { MembersPanel } from "./members-panel";
import { SettingsNav } from "../nav";
import { loadSettingsCtx } from "../_shared";

/// Members tab — lists every account that can log into the workspace.
/// Admins can invite new users, flip roles between admin/editor/viewer,
/// suspend accounts, and set per-user quotas.  Everyone else gets a
/// read-only roster.
export default async function SettingsMembersPage() {
  const { cookieHeader, role } = await loadSettingsCtx();
  const [members, systems] = await Promise.all([
    safeMembers(cookieHeader),
    safeSystems(cookieHeader),
  ]);
  return (
    <div className="scr">
      <Sidebar nav="settings" systems={systems} />
      <TopBar crumbs={["Settings", "Members"]} title={`Members · ${members.length}`} />
      <div className="main split-rail" style={{ overflow: "auto" }}>
        <SettingsNav active="members" />
        <div style={{ overflow: "auto", padding: "24px 32px" }}>
          <div style={{ maxWidth: 960 }}>
            <div className="t-3xl t-semibold">Members</div>
            <div className="t-sm t-muted" style={{ marginTop: 4, marginBottom: 24 }}>
              Workspace accounts · {members.length} total · admins can invite + change roles
            </div>
            <MembersPanel initial={members} canMutate={role === "admin"} />
          </div>
        </div>
      </div>
    </div>
  );
}
