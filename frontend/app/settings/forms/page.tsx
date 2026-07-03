import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeFormsAdmin, safeSystems, safeWorkflowTemplates } from "@/lib/api";
import { canMutate } from "@/lib/roles";

import { FormsPanel } from "./forms-panel";
import { SettingsNav } from "../nav";
import { loadSettingsCtx } from "../_shared";

/// Request forms tab — the Form Designer. Admins/editors define the request
/// types people submit in the everyday app: each form's fields and the workflow
/// template that routes it for approval. The everyday flow reads these live.
export default async function SettingsFormsPage() {
  const { cookieHeader, role } = await loadSettingsCtx();
  const [forms, templates, systems] = await Promise.all([
    safeFormsAdmin(cookieHeader),
    safeWorkflowTemplates(cookieHeader),
    safeSystems(cookieHeader),
  ]);
  return (
    <div className="scr">
      <Sidebar nav="settings" systems={systems} />
      <TopBar crumbs={["Settings", "Request forms"]} title="Request forms" />
      <div className="main split-rail" style={{ overflow: "auto" }}>
        <SettingsNav active="forms" />
        <div style={{ overflow: "auto", padding: "24px 32px" }}>
          <div className="page-body">
            <div className="t-3xl t-semibold">Request forms</div>
            <div className="t-sm t-muted" style={{ marginTop: 4, marginBottom: 24 }}>
              Design the request types people submit in the everyday app — each form&apos;s fields and the
              approval route it follows. Changes go live immediately; the AI classifies requests into these forms.
            </div>
            <FormsPanel initial={forms} templates={templates} canMutate={canMutate(role)} />
          </div>
        </div>
      </div>
    </div>
  );
}
