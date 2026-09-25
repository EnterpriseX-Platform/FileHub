import type { Metadata } from "next";
import { headers } from "next/headers";

import { AccessDenied } from "@/components/access-denied";
import { AuthProvider } from "@/lib/auth-context";
import { accentCss, loadBranding } from "@/lib/branding-server";
import { SidebarProvider } from "@/lib/sidebar-context";
import { ThemeProvider } from "@/lib/theme-context";

import "./tokens.css";

/// Title comes from the workspace branding (Settings → General), not the code.
export async function generateMetadata(): Promise<Metadata> {
  const b = await loadBranding();
  const title = b.org_name ? `${b.workspace_display} · ${b.org_name}` : b.workspace_display;
  return { title, description: `${b.workspace_display} — central file storage` };
}

// Runs before first paint so a dark-mode user never sees a white flash.
// Inline (not next/script) — external scripts load too late and interact
// badly with basePath. Mirrors lib/theme-context.tsx, which reads the class
// this sets.
// Light by default (embedding portals are usually light); users can switch.
const THEME_BOOT = `try{if(localStorage.getItem("fh-theme")==="dark")document.documentElement.classList.add("dark")}catch(e){}`;

/// Ask the backend whether this visitor may use the app: 403 = signed in
/// through the identity proxy but without a File Hub role.  401 passes
/// through (login page / share links have no identity yet).
async function edgeAccess(): Promise<{ denied: boolean; email?: string }> {
  const backend = process.env.BACKEND_URL || "http://127.0.0.1:8090";
  try {
    const h = await headers();
    // The console uses its own login unless the proxy marks this request as
    // SSO (x-filehub-ui-sso), in which case the proxy identity is checked here.
    if (!h.get("x-filehub-ui-sso")) return { denied: false };
    const fwd: Record<string, string> = {};
    for (const k of [
      "cookie",
      "x-filehub-edge",
      "x-forwarded-access-token",
      "x-auth-request-email",
      "x-auth-request-preferred-username",
    ]) {
      const v = h.get(k);
      if (v) fwd[k] = v;
    }
    const r = await fetch(`${backend}/fh/api/auth/me`, { headers: fwd, cache: "no-store" });
    if (r.status === 403) {
      return { denied: true, email: h.get("x-auth-request-email") ?? undefined };
    }
  } catch {
    // Backend unreachable is not an access problem — let the page handle it.
  }
  return { denied: false };
}

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  const [access, brand] = await Promise.all([edgeAccess(), loadBranding()]);
  const accent = accentCss(brand.brand_accent);
  return (
    // suppressHydrationWarning: the boot script mutates <html> className
    // before React hydrates, which is exactly the mismatch React warns about.
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_BOOT }} />
        {accent && <style dangerouslySetInnerHTML={{ __html: accent }} />}
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="" />
        {/* IBM Plex Sans Thai is the official Thai companion family — Latin
            resolves from Plex Sans first, Thai glyphs fall through to it
            (system fallback would mismatch baseline/weight). */}
        {/* eslint-disable-next-line @next/next/no-page-custom-font -- the rule
            targets Pages Router _document.js; this App Router root layout IS
            the app-wide document, so the font loads on every page. */}
        <link
          rel="stylesheet"
          href="https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;500;600;700&family=IBM+Plex+Sans+Thai:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap"
        />
      </head>
      <body>
        <ThemeProvider>
          {access.denied ? (
            <AccessDenied email={access.email} branding={brand} />
          ) : (
            <AuthProvider><SidebarProvider>{children}</SidebarProvider></AuthProvider>
          )}
        </ThemeProvider>
      </body>
    </html>
  );
}
