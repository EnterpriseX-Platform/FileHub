import { NewRequestClient } from "@/components/everyday/new-request-client";
import { safeRequestForms } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";

export const dynamic = "force-dynamic";

export default async function NewRequestPage() {
  const { cookieHeader, role } = await loadServerCtx();
  const forms = await safeRequestForms(cookieHeader);
  return <NewRequestClient forms={forms} role={role} />;
}
