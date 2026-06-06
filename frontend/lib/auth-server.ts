import { cookies } from "next/headers";

// IPv4 default — see lib/api.ts for the macOS `::1` story.
const BACKEND = process.env.BACKEND_URL || "http://127.0.0.1:8090";

/// Server-component helper: returns the inbound cookie header (for forwarding
/// to the Rust backend's private routes) plus the signed-in user's role (for
/// gating admin-only mutations server-side).  Every server-rendered page that
/// reads from a private `/fh/api/*` endpoint needs the cookie or the request
/// gets a 401 and the helpers swallow it as an empty result.
export async function loadServerCtx(): Promise<{ cookieHeader: string; role: string | null }> {
  const cookieHeader = (await cookies()).getAll().map((c) => `${c.name}=${c.value}`).join("; ");
  const meRes = await fetch(`${BACKEND}/fh/api/auth/me`, {
    headers: cookieHeader ? { cookie: cookieHeader } : undefined,
    cache: "no-store",
  }).catch(() => null);
  const me: { role?: string } | null = meRes && meRes.ok ? await meRes.json() : null;
  return { cookieHeader, role: me?.role ?? null };
}
