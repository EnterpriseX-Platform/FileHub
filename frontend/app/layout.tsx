import type { Metadata } from "next";

import { AuthProvider } from "@/lib/auth-context";
import { SidebarProvider } from "@/lib/sidebar-context";
import { ThemeProvider } from "@/lib/theme-context";

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

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    // suppressHydrationWarning: the boot script mutates <html> className
    // before React hydrates, which is exactly the mismatch React warns about.
    <html lang="en" suppressHydrationWarning>
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
        <ThemeProvider><AuthProvider><SidebarProvider>{children}</SidebarProvider></AuthProvider></ThemeProvider>
      </body>
    </html>
  );
}
