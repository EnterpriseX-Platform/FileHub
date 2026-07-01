"use client";

import { usePathname, useRouter } from "next/navigation";
import * as React from "react";

import { GlobalSearch } from "@/components/global-search";
import { Ico } from "@/components/icons";
import { NotificationsBell } from "@/components/notifications-bell";
import { Av } from "@/components/primitives";
import { useAuth } from "@/lib/auth-context";
import { useI18n } from "@/lib/i18n";
import { useTheme } from "@/lib/theme-context";
import { canMutate, isAdmin } from "@/lib/roles";
import { useViewMode } from "@/lib/view-mode";

/// The Everyday (consumer) app shell — a clean top bar over a centered content
/// column, deliberately different from the Workspace sidebar so the two
/// personas feel distinct. Chrome is shared (⌘K palette, notifications, theme).
const NAV = [
  { href: "/home", labelKey: "eday.nav.home" },
  { href: "/my",   labelKey: "eday.nav.myfiles" },
];

export function EverydayShell({ children }: { children: React.ReactNode }) {
  const { user } = useAuth();
  const { t } = useI18n();
  const pathname = usePathname();
  return (
    <div className="eday">
      <header className="eday-top">
        <a href="/home" className="eday-brand" aria-label="Home">
          <span className="ws-logo" style={{ background: "var(--grad)" }}>F</span>
          <span className="eday-brand-name">File Hub</span>
        </a>
        <nav className="eday-nav">
          {NAV.map((n) => (
            <a key={n.href} href={n.href} className={"eday-navlink" + (pathname === n.href ? " active" : "")}>
              {t(n.labelKey)}
            </a>
          ))}
        </nav>
        <div style={{ flex: 1 }} />
        <div className="eday-search"><GlobalSearch /></div>
        {canMutate(user?.role ?? null) && (
          <a className="btn primary sm eday-upload" href="/upload"><Ico.upload className="icon sm" /> <span>Upload</span></a>
        )}
        <NotificationsBell tone="ghost" />
        <EdayUserMenu />
      </header>
      <main className="eday-main">{children}</main>
    </div>
  );
}

function EdayUserMenu() {
  const { user, logout } = useAuth();
  const { dark, toggle: toggleTheme } = useTheme();
  const { setMode } = useViewMode();
  const router = useRouter();
  const [open, setOpen] = React.useState(false);
  const ref = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) setOpen(false); };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => { document.removeEventListener("mousedown", onDown); document.removeEventListener("keydown", onKey); };
  }, [open]);

  if (!user) return <a href="/login" className="btn sm primary">Sign in</a>;

  const toFullView = () => { setMode("workspace"); router.push("/"); };

  return (
    <div style={{ position: "relative" }} ref={ref}>
      <button className="eday-avatarbtn" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        <Av name={user.display_name} tone={tone(user.avatar_tone)} />
      </button>
      {open && (
        <div className="card eday-menu" role="menu">
          <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 8px" }}>
            <Av name={user.display_name} tone={tone(user.avatar_tone)} />
            <div style={{ minWidth: 0 }}>
              <div className="t-sm t-semibold t-trunc">{user.display_name}</div>
              <div className="t-xs t-subtle t-trunc">{user.email}</div>
            </div>
          </div>
          <div className="divider" style={{ margin: "4px 0" }} />
          <button onClick={toggleTheme} className="btn xs ghost eday-menu-item" role="menuitemcheckbox" aria-checked={dark}>
            <Ico.moon className="icon sm" /> Dark mode
            <span className="t-xs t-subtle" style={{ marginLeft: "auto" }}>{dark ? "on" : "off"}</span>
          </button>
          {/* Only admins/editors have a full view worth switching to. */}
          {(isAdmin(user.role) || canMutate(user.role)) && (
            <button onClick={toFullView} className="btn xs ghost eday-menu-item" role="menuitem">
              <Ico.cog className="icon sm" /> Switch to full view
            </button>
          )}
          <button onClick={() => { setOpen(false); logout(); }} className="btn xs ghost eday-menu-item" role="menuitem">
            <Ico.x className="icon sm" /> Sign out
          </button>
        </div>
      )}
    </div>
  );
}

function tone(t: string): "indigo" | "emerald" | "amber" | "rose" | "violet" | "cyan" | "fuchsia" | "slate" {
  const allowed = ["indigo", "emerald", "amber", "rose", "violet", "cyan", "fuchsia", "slate"] as const;
  return (allowed as readonly string[]).includes(t) ? (t as typeof allowed[number]) : "slate";
}
