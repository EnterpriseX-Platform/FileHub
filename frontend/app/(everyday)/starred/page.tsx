import { MyFilesClient } from "@/components/everyday/my-files-client";
import { safeStarred } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { canMutate } from "@/lib/roles";

/// Everyday "Starred" — the caller's favorites (GET /api/starred).
export default async function StarredPage() {
  const { cookieHeader, role } = await loadServerCtx();
  const files = await safeStarred(cookieHeader);
  return (
    <MyFilesClient
      files={files}
      titleKey="eday.starredTitle"
      emptyKey="eday.noStarred"
      canUpload={canMutate(role)}
    />
  );
}
