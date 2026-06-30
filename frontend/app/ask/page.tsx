import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeOrgs, safeStats, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";

import { AskClient } from "./ask-client";

export default async function AskPage() {
  const { cookieHeader } = await loadServerCtx();
  const [systems, stats] = await Promise.all([
    safeSystems(cookieHeader),
    safeStats(cookieHeader),
  ]);
  const orgs = systems[0] ? await safeOrgs(systems[0].id, cookieHeader) : [];

  return (
    <div className="scr">
      <Sidebar nav="ask" systems={systems} stats={stats} orgs={orgs} />
      <TopBar crumbs={["Workspace", "Ask"]} />
      <div className="main main-pad" style={{ overflow: "auto" }}>
        <AskClient />
      </div>
    </div>
  );
}
