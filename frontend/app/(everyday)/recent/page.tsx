import { MyFilesClient } from "@/components/everyday/my-files-client";
import { safeFiles } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { canMutate } from "@/lib/roles";

/// Everyday "Recent" — the most recently changed files the user can access.
/// (A true "recently viewed by me" needs per-user view tracking; deferred with
/// Starred. Recency-by-modified is a solid first cut.)
export default async function RecentPage() {
  const { cookieHeader, role } = await loadServerCtx();
  const all = await safeFiles({}, cookieHeader);
  const files = [...all]
    .sort((a, b) => (b.modified_at || "").localeCompare(a.modified_at || ""))
    .slice(0, 50);
  return <MyFilesClient files={files} titleKey="eday.recent" canUpload={canMutate(role)} />;
}
