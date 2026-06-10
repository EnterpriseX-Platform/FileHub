import type { Metadata } from "next";

import { AuthProvider } from "@/lib/auth-context";
import { SidebarProvider } from "@/lib/sidebar-context";

import "./tokens.css";

export const metadata: Metadata = {
  title: "File Hub",
  description: "DevOps file management with Notion-style metadata views",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="" />
        {/* IBM Plex Sans Thai is the official Thai companion family — Latin
            resolves from Plex Sans first, Thai glyphs fall through to it
            (system fallback would mismatch baseline/weight). */}
        <link
          rel="stylesheet"
          href="https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;500;600;700&family=IBM+Plex+Sans+Thai:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap"
        />
      </head>
      <body>
        <AuthProvider><SidebarProvider>{children}</SidebarProvider></AuthProvider>
      </body>
    </html>
  );
}
