import { MyFilesClient } from "@/components/everyday/my-files-client";
import { safeStarred, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { canMutate } from "@/lib/roles";

/// Everyday "Starred" — the caller's favorites (GET /api/starred).
export default async function StarredPage() {
  const { cookieHeader, role } = await loadServerCtx();
  const [files, systems] = await Promise.all([safeStarred(cookieHeader), safeSystems(cookieHeader)]);
  const areas = systems.filter((s) => s.system_type !== "personal").map((s) => ({ id: s.id, name: s.name, tone: s.tone }));
  return (
    <MyFilesClient
      files={files}
      titleKey="eday.starredTitle"
      emptyKey="eday.noStarred"
      canUpload={canMutate(role)}
      areas={areas}
    />
  );
}
