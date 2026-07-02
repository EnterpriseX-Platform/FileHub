import { HomeClient } from "@/components/everyday/home-client";
import { safeFiles, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { canMutate } from "@/lib/roles";

/// Everyday Home — the consumer landing. Server-fetches the same data as the
/// Workspace dashboard, then hands plain rows to a client view so everything
/// localises through useI18n.
export default async function EverydayHome() {
  const { cookieHeader, role } = await loadServerCtx();
  const canUpload = canMutate(role);
  const [allFiles, reviewFiles, systems] = await Promise.all([
    safeFiles({}, cookieHeader),
    canUpload ? safeFiles({ status: "Review", limit: "6" }, cookieHeader) : Promise.resolve([]),
    safeSystems(cookieHeader),
  ]);

  const recent = [...allFiles]
    .sort((a, b) => (b.modified_at || "").localeCompare(a.modified_at || ""))
    .slice(0, 8);
  const countBySystem = new Map<string, number>();
  for (const f of allFiles) countBySystem.set(f.system_id, (countBySystem.get(f.system_id) ?? 0) + 1);
  const areas = systems
    .filter((s) => s.system_type !== "personal")
    .map((s) => ({ id: s.id, name: s.name, tone: s.tone, count: countBySystem.get(s.id) ?? 0 }));

  return <HomeClient recent={recent} review={reviewFiles} areas={areas} canUpload={canUpload} />;
}
