import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";

import { SettingsNav } from "../nav";

/// Roles & permissions — read-only matrix that mirrors `require_role` calls
/// in the Rust backend.  Source of truth lives in code, this page is the
/// human-readable view so admins know what each role can do without
/// reading source.
const CAPS: Array<[string, "admin" | "editor" | "viewer", "admin" | "editor" | "viewer"]> = [
  // [capability, minimum role to do it, minimum role to merely view it]
  ["View files + folders",                  "viewer", "viewer"],
  ["Upload + download files",               "editor", "viewer"],
  ["Edit metadata / move / rename",         "editor", "viewer"],
  ["Soft-delete to Trash",                  "editor", "viewer"],
  ["Restore from Trash",                    "editor", "viewer"],
  ["Create folders + saved views",          "editor", "viewer"],
  ["Approve / reject workflow steps",       "editor", "viewer"],
  ["Comment on files",                      "viewer", "viewer"],
  ["Create share links",                    "editor", "viewer"],
  ["Hard-delete from Trash",                "admin",  "viewer"],
  ["Create / edit / delete buckets",        "admin",  "viewer"],
  ["Edit rotation policies",                "admin",  "editor"],
  ["Invite / suspend members",              "admin",  "editor"],
  ["Change per-user or per-org quota",      "admin",  "editor"],
  ["Edit workspace identity + access flags","admin",  "editor"],
  ["Run rotation engine manually",          "admin",  "editor"],
];

export default async function SettingsRolesPage() {
  const { cookieHeader } = await loadServerCtx();
  const systems = await safeSystems(cookieHeader);
  return (
    <div className="scr">
      <Sidebar nav="settings" systems={systems} />
      <TopBar crumbs={["Settings", "Roles & permissions"]} title="Roles & permissions" />
      <div className="main split-rail" style={{ overflow: "auto" }}>
        <SettingsNav active="roles" />
        <div className="main-pad" style={{ overflow: "auto" }}>
          <div className="page-body">
            <div className="t-3xl t-semibold">Roles & permissions</div>
            <div className="t-sm t-muted" style={{ marginTop: 4, marginBottom: 24 }}>
              Three built-in roles, enforced by the server on every request.
              Change a user&apos;s role from the Members tab.
            </div>

            <div className="legend-grid" style={{ marginBottom: 20 }}>
              <RoleCard role="admin"  tone="indigo"  icon={<Ico.shield />}
                        blurb="Full mutation rights — bucket CRUD, members, rotation, workspace settings." />
              <RoleCard role="editor" tone="emerald" icon={<Ico.upload />}
                        blurb="Upload, edit, move, approve workflows. Cannot change buckets or members." />
              <RoleCard role="viewer" tone="slate"   icon={<Ico.eye />}
                        blurb="Read-only. Can browse + comment but cannot mutate." />
            </div>

            <div className="card" style={{ padding: 0 }}>
              <div className="table-scroll">
              <table className="tbl">
                <thead>
                  <tr>
                    <th scope="col">Capability</th>
                    <th scope="col" style={{ width: 110, textAlign: "center" }}>Admin</th>
                    <th scope="col" style={{ width: 110, textAlign: "center" }}>Editor</th>
                    <th scope="col" style={{ width: 110, textAlign: "center" }}>Viewer</th>
                  </tr>
                </thead>
                <tbody>
                  {CAPS.map(([cap, mut]) => (
                    <tr key={cap}>
                      <td className="t-sm">{cap}</td>
                      <td style={{ textAlign: "center" }}>{can("admin",  mut) ? "✓" : "—"}</td>
                      <td style={{ textAlign: "center" }}>{can("editor", mut) ? "✓" : "—"}</td>
                      <td style={{ textAlign: "center" }}>{can("viewer", mut) ? "✓" : "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function RoleCard({ role, tone, icon, blurb }: { role: string; tone: "indigo" | "emerald" | "slate"; icon: React.ReactNode; blurb: string }) {
  return (
    <div className="card" style={{ padding: 14 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
        <Pill tone={tone}>{icon}{role}</Pill>
      </div>
      <div className="t-xs t-muted">{blurb}</div>
    </div>
  );
}

function can(role: "admin" | "editor" | "viewer", required: "admin" | "editor" | "viewer"): boolean {
  const order = { viewer: 0, editor: 1, admin: 2 };
  return order[role] >= order[required];
}
