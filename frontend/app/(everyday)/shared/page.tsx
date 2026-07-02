import { MyFilesClient } from "@/components/everyday/my-files-client";
import { safeFiles, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { canMutate } from "@/lib/roles";

/// Everyday "Shared with you" — accessible files that someone else owns. This
/// is a proxy (owner ≠ me) since there's no dedicated "shared with me" endpoint
/// yet; good enough for the everyday view, refine when share-graph data lands.
export default async function SharedPage() {
  const { cookieHeader, role, name } = await loadServerCtx();
  const [all, systems] = await Promise.all([safeFiles({}, cookieHeader), safeSystems(cookieHeader)]);
  const files = all
    .filter((f) => name && f.owner && f.owner !== name)
    .sort((a, b) => (b.modified_at || "").localeCompare(a.modified_at || ""));
  const areas = systems.filter((s) => s.system_type !== "personal").map((s) => ({ id: s.id, name: s.name, tone: s.tone }));
  return <MyFilesClient files={files} titleKey="eday.sharedTitle" canUpload={canMutate(role)} areas={areas} />;
}
