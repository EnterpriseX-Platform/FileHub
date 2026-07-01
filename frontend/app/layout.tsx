import type { Metadata } from "next";
import { cookies } from "next/headers";

import { MobileUploadFab } from "@/components/mobile-upload-fab";
import { AuthProvider } from "@/lib/auth-context";
import { I18nProvider, type Locale } from "@/lib/i18n";
import { SidebarProvider } from "@/lib/sidebar-context";
import { ThemeProvider } from "@/lib/theme-context";
import { ViewModeProvider, type ViewMode } from "@/lib/view-mode";

import "./tokens.css";

export const metadata: Metadata = {
  title: "File Hub",
  description: "DevOps file management with Notion-style metadata views",
};

// Runs before first paint so a dark-mode user never sees a white flash.
// Inline (not next/script) — external scripts load too late and interact
// badly with basePath. Mirrors lib/theme-context.tsx, which reads the class
// this sets.
const THEME_BOOT = `try{var t=localStorage.getItem("fh-theme");var d=t?t==="dark":matchMedia("(prefers-color-scheme: dark)").matches;if(d)document.documentElement.classList.add("dark")}catch(e){}`;

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  // Read the locale cookie server-side so the first render is already in the
  // user's language (no flash, no hydration mismatch on translated text).
  const jar = await cookies();
  const locale: Locale = jar.get("fh-locale")?.value === "th" ? "th" : "en";
  const viewCookie = jar.get("fh-view")?.value;
  const initialMode: ViewMode | null =
    viewCookie === "everyday" || viewCookie === "workspace" ? viewCookie : null;
  return (
    // suppressHydrationWarning: the boot script mutates <html> className
    // before React hydrates, which is exactly the mismatch React warns about.
    <html lang={locale} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_BOOT }} />
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
        <I18nProvider initialLocale={locale}><ThemeProvider><AuthProvider><ViewModeProvider initialMode={initialMode}><SidebarProvider>{children}<MobileUploadFab /></SidebarProvider></ViewModeProvider></AuthProvider></ThemeProvider></I18nProvider>
      </body>
    </html>
  );
}
