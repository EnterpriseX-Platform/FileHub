import { NewRequestClient } from "@/components/everyday/new-request-client";
import { safeMembers, safeRequestForms } from "@/lib/api";
import { loadServerCtx } from "@/lib/auth-server";

export const dynamic = "force-dynamic";

export default async function NewRequestPage() {
  const { cookieHeader, role } = await loadServerCtx();
  const [forms, members] = await Promise.all([
    safeRequestForms(cookieHeader),
    safeMembers(cookieHeader),
  ]);
  return <NewRequestClient forms={forms} members={members} role={role} />;
}
