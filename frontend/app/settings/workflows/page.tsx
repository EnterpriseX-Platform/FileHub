import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeMembers, safeSystems, safeWorkflowTemplates } from "@/lib/api";
import { canMutate } from "@/lib/roles";

import { WorkflowsPanel } from "./workflows-panel";
import { SettingsNav } from "../nav";
import { loadSettingsCtx } from "../_shared";

/// Workflow templates tab — reusable no-code approval flows (TOR Annex A).
/// An editor defines an ordered list of review steps once; anyone starting a
/// workflow on a file can pick the template instead of hand-picking reviewers.
export default async function SettingsWorkflowsPage() {
  const { cookieHeader, role } = await loadSettingsCtx();
  const [templates, members, systems] = await Promise.all([
    safeWorkflowTemplates(cookieHeader),
    safeMembers(cookieHeader),
    safeSystems(cookieHeader),
  ]);
  return (
    <div className="scr">
      <Sidebar nav="settings" systems={systems} />
      <TopBar crumbs={["Settings", "Workflow templates"]} title="Workflow templates" />
      <div className="main split-rail" style={{ overflow: "auto" }}>
        <SettingsNav active="workflows" />
        <div style={{ overflow: "auto", padding: "24px 32px" }}>
          <div style={{ maxWidth: 760 }}>
            <div className="t-3xl t-semibold">Workflow templates</div>
            <div className="t-sm t-muted" style={{ marginTop: 4, marginBottom: 24 }}>
              Define an approval route once — named steps, each assigned to a reviewer,
              run in order or in parallel — then start it from any file&apos;s workflow panel.
            </div>
            <WorkflowsPanel initial={templates} members={members} canMutate={canMutate(role)} />
          </div>
        </div>
      </div>
    </div>
  );
}
