import { RequestsClient } from "@/components/everyday/requests-client";
import { safeRequests } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";

export const dynamic = "force-dynamic";

export default async function RequestsPage() {
  const { cookieHeader } = await loadServerCtx();
  const [inbox, mine] = await Promise.all([
    safeRequests("inbox", cookieHeader),
    safeRequests("mine", cookieHeader),
  ]);
  return <RequestsClient inbox={inbox} mine={mine} />;
}
