import type { Metadata } from "next";
import { headers } from "next/headers";

import { AccessDenied } from "@/components/access-denied";
import { AuthProvider } from "@/lib/auth-context";
import { SidebarProvider } from "@/lib/sidebar-context";
import { ThemeProvider } from "@/lib/theme-context";

import "./tokens.css";

export const metadata: Metadata = {
  title: "คลังไฟล์กลาง NEB",
  description: "คลังไฟล์กลางของระบบ New e-Budgeting",
};

// Runs before first paint so a dark-mode user never sees a white flash.
// Inline (not next/script) — external scripts load too late and interact
// badly with basePath. Mirrors lib/theme-context.tsx, which reads the class
// this sets.
// NEB: จอของระบบอื่นในพอร์ทัลเป็นโทนสว่างทั้งหมด ถ้าปล่อยตามค่าเครื่องผู้ใช้
// จะเปิดมาเจอจอดำที่ดูหลุดจากระบบ ⇒ เริ่มต้นสว่างเสมอ แล้วให้สลับเองได้
const THEME_BOOT = `try{if(localStorage.getItem("fh-theme")==="dark")document.documentElement.classList.add("dark")}catch(e){}`;

/// ถามหลังบ้านว่าผู้ใช้คนนี้เข้าได้ไหม — 403 = ล็อกอินแล้วแต่ไม่มีสิทธิ์
/// (401 ปล่อยผ่าน เพราะเป็นเส้นทางล็อกอิน/ลิงก์แชร์ที่ยังไม่มีตัวตน)
async function edgeAccess(): Promise<{ denied: boolean; email?: string }> {
  const backend = process.env.BACKEND_URL || "http://127.0.0.1:8090";
  try {
    const h = await headers();
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
    // ต่อหลังบ้านไม่ได้ = ไม่ใช่เรื่องสิทธิ์ ปล่อยให้จอปกติจัดการต่อ
  }
  return { denied: false };
}

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  const access = await edgeAccess();
  return (
    // suppressHydrationWarning: the boot script mutates <html> className
    // before React hydrates, which is exactly the mismatch React warns about.
    <html lang="th" suppressHydrationWarning>
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
        <ThemeProvider>
          {access.denied ? (
            <AccessDenied email={access.email} />
          ) : (
            <AuthProvider><SidebarProvider>{children}</SidebarProvider></AuthProvider>
          )}
        </ThemeProvider>
      </body>
    </html>
  );
}
