import { FileViewClient } from "@/components/everyday/file-view-client";
import { safeFile, safeSystems } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";

/// Everyday file view. Reachable at /f/<id>; the everyday file links point here
/// instead of the technical /files/<id> detail page.
export default async function EverydayFilePage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const { cookieHeader } = await loadServerCtx();
  const file = await safeFile(id, cookieHeader);

  if (!file) {
    return (
      <div style={{ padding: "48px 0", textAlign: "center" }} className="t-sm t-subtle">
        This file isn&apos;t available.
      </div>
    );
  }

  const systems = await safeSystems(cookieHeader);
  const areaName = systems.find((s) => s.id === file.system_id)?.name ?? null;

  return <FileViewClient file={file} areaName={areaName} />;
}
