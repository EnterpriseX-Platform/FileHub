import { MyFilesClient } from "@/components/everyday/my-files-client";
import { safeFiles, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";
import { canMutate } from "@/lib/roles";

/// Everyday "My files" — a plain browser over the user's accessible files.
/// Filtering (area / status) is done here in-memory, matching the app's
/// frontend-filtering convention; the client handles view toggle + in-page
/// search. Deliberately minimal columns: name · size · modified.
export default async function MyFilesPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | undefined>>;
}) {
  const sp = await searchParams;
  const { cookieHeader, role } = await loadServerCtx();
  const [all, systems] = await Promise.all([
    safeFiles({}, cookieHeader),
    safeSystems(cookieHeader),
  ]);

  let files = all;
  let title: string | undefined;
  let titleKey: string | undefined = "eday.nav.myfiles";
  if (sp.area) {
    files = all.filter((f) => f.system_id === sp.area);
    title = systems.find((s) => s.id === sp.area)?.name ?? "Area";
    titleKey = undefined;
  } else if (sp.status) {
    files = all.filter((f) => f.status === sp.status);
    titleKey = sp.status === "Review" ? "eday.needsReview" : undefined;
    title = sp.status === "Review" ? undefined : sp.status;
  }
  files = [...files].sort((a, b) => (b.modified_at || "").localeCompare(a.modified_at || ""));

  return <MyFilesClient files={files} titleKey={titleKey} title={title} canUpload={canMutate(role)} />;
}
