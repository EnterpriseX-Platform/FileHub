import { cookies } from "next/headers";

import { EverydayShell } from "@/components/everyday/shell";
import { Sidebar } from "@/components/sidebar";
import { TopBar } from "@/components/topbar";
import { safeOrgs, safeStats, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";

import { AskClient } from "./ask-client";

/// Ask serves both shells: it's a first-class everyday page (top-nav tab,
/// ✦ pill, home hero) and must never pull users into the Admin console.
export default async function AskPage() {
  const { cookieHeader } = await loadServerCtx();

  const viewCookie = (await cookies()).get("fh-view")?.value;
  if ((viewCookie ?? "everyday") === "everyday") {
    return (
      <EverydayShell>
        <AskClient />
      </EverydayShell>
    );
  }

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
