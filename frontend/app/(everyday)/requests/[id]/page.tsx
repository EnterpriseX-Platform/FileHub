import { notFound } from "next/navigation";

import { RequestDetailClient } from "@/components/everyday/request-detail-client";
import { safeRequest } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";

export const dynamic = "force-dynamic";

export default async function RequestDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const { cookieHeader } = await loadServerCtx();
  const req = await safeRequest(id, cookieHeader);
  if (!req) notFound();
  return <RequestDetailClient req={req} />;
}
