"use client";

import * as React from "react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";

import { useAuth } from "@/lib/auth-context";
import { api, bytes, type Basket, type VBasket } from "@/lib/fh";
import { ThemeSwitch } from "@/lib/theme-context";
import { useBranding } from "@/lib/branding";
import { basketColor, I, Mark, ToastHost } from "./ui";

type NavData = {
  baskets: Basket[];
  drive: Basket | null;
  vbaskets: VBasket[];
  downloads: number;
  unread: number;
  quota: { used: number; cap: number } | null;
  /// "basket:<id>" / "vbasket:<id>" the person starred.
  stars: string[];
  reload: () => void;
  setDownloads: (n: number) => void;
  toggleStar: (kind: "basket" | "vbasket", id: string) => void;
};
const NavCtx = React.createContext<NavData>({ baskets: [], drive: null, vbaskets: [], downloads: 0, unread: 0, quota: null, stars: [], reload: () => {}, setDownloads: () => {}, toggleStar: () => {} });
export function useNav() { return React.useContext(NavCtx); }

/// Carbon UI shell: black header strip, white side nav (Recent · My Drive ·
/// Shared with me · Awaiting my review · BASKETS tree · VIRTUAL BASKETS ·
/// Download list · Trash · Admin), content on the gray background.
export function Shell({ children }: { children: React.ReactNode }) {
  const [navOpen, setNavOpen] = React.useState(false);
  const [navHidden, setNavHidden] = React.useState(false);
  const [data, setData] = React.useState<Omit<NavData, "reload" | "setDownloads" | "toggleStar">>({ baskets: [], drive: null, vbaskets: [], downloads: 0, unread: 0, quota: null, stars: [] });
  const [tick, setTick] = React.useState(0);
  const pathname = usePathname();
  React.useEffect(() => setNavOpen(false), [pathname]);

  React.useEffect(() => {
    let off = false;
    (async () => {
      const [b, d, v, dl, un, st] = await Promise.all([
        api<Basket[]>("/systems").catch(() => []),
        api<Basket>("/personal-drive").catch(() => null),
        api<VBasket[]>("/vbaskets").catch(() => []),
        api<{ files: number }[]>("/me/downloads").catch(() => []),
        api<{ count?: number; unread?: number } | number>("/notifications/unread-count").catch(() => 0),
        api<{ kind: string; id: string }[]>("/me/stars").catch(() => []),
      ]);
      const unread = typeof un === "number" ? un : (un.count ?? un.unread ?? 0);
      let quota: NavData["quota"] = null;
      if (d) {
        const q = await api<{ scopes: { scope: string; id: string | null; used_bytes: number; limit_bytes: number }[] }>("/quota").catch(() => null);
        const s = q?.scopes.find((x) => x.scope === "user") ?? q?.scopes.find((x) => x.id === d.id);
        quota = s ? { used: s.used_bytes, cap: s.limit_bytes || d.quota_bytes || 0 } : null;
      }
      if (!off) setData({ baskets: b.filter((x) => x.system_type !== "personal"), drive: d, vbaskets: v, downloads: dl.length, unread, quota, stars: st.map((x) => `${x.kind}:${x.id}`) });
    })();
    return () => { off = true; };
  }, [tick]);

  const value = React.useMemo(() => ({
    ...data,
    reload: () => setTick((t) => t + 1),
    setDownloads: (n: number) => setData((x) => ({ ...x, downloads: n })),
    toggleStar: (kind: "basket" | "vbasket", id: string) => {
      const key = `${kind}:${id}`;
      const on = data.stars.includes(key);
      setData((x) => ({ ...x, stars: on ? x.stars.filter((k) => k !== key) : [...x.stars, key] }));
      api(`/me/stars/${kind}/${encodeURIComponent(id)}`, { method: on ? "DELETE" : "PUT" })
        .catch(() => setData((x) => ({ ...x, stars: on ? [...x.stars, key] : x.stars.filter((k) => k !== key) })));
    },
  }), [data]);

  return (
    <NavCtx.Provider value={value}>
      <ToastHost>
        <div className="cx-app" data-nav-open={navOpen} data-nav-hidden={navHidden || undefined}>
          <Header onMenu={() => { if (window.matchMedia("(max-width: 1055px)").matches) setNavOpen((o) => !o); else setNavHidden((h) => !h); }} />
          <SideNav />
          {navOpen && <div className="cx-nav-scrim" onClick={() => setNavOpen(false)} />}
          <div className="cx-main">
            {children}
          </div>
        </div>
      </ToastHost>
    </NavCtx.Provider>
  );
}

/// UI shell header: menu, product name, where you are; notifications and you on the right.
function Header({ onMenu }: { onMenu: () => void }) {
  const nav = useNav();
  const { user, logout } = useAuth();
  const pathname = usePathname() || "";
  const [me, setMe] = React.useState(false);
  const brandName = useBranding().brand_name;
  const here = React.useMemo(() => {
    const id = pathname.split("/")[2];
    if (pathname.startsWith("/b/")) return nav.drive?.id === id ? "My Drive" : nav.baskets.find((b) => b.id === id)?.name ?? "";
    if (pathname === "/v/new") return "New virtual basket";
    if (pathname.startsWith("/v/")) return nav.vbaskets.find((v) => v.id === id)?.name ?? "";
    if (pathname.startsWith("/admin/baskets/")) return nav.baskets.find((b) => b.id === pathname.split("/")[3])?.name ?? "Admin";
    if (pathname.startsWith("/view/")) return "File";
    const names: Record<string, string> = { recent: "Recent", notifications: "Notifications", shared: "Shared with me", review: "Awaiting my review",
      baskets: "All baskets", downloads: "Download list", trash: "Trash", reports: "Reports", docs: "API docs", admin: "Admin", search: "Search" };
    return names[pathname.split("/")[1]] ?? "";
  }, [pathname, nav]);
  return (
    <header className="cx-header">
      <button className="cx-header-btn" aria-label="Menu" onClick={onMenu}><I.menu size={20} /></button>
      <Link href="/recent" className="cx-header-brand"><Mark /><span>{brandName}</span></Link>
      {here && <><span className="cx-header-div" /><span className="cx-header-here">{here}</span></>}
      <span className="cx-spacer" />
      <Link href="/search" className="cx-header-btn" aria-label="Search"><I.search size={20} /></Link>
      <Link href="/notifications" className="cx-header-btn" aria-label={nav.unread ? `Notifications, ${nav.unread} unread` : "Notifications"}>
        <I.bell size={20} />{nav.unread > 0 && <i className="cx-header-dot" />}
      </Link>
      {user && (
        <div style={{ position: "relative" }}>
          <button className="cx-header-btn" aria-label={user.display_name} aria-haspopup="menu" aria-expanded={me} onClick={() => setMe((o) => !o)}>
            <span className="cx-avatar">{user.display_name.trim().charAt(0).toUpperCase()}</span>
          </button>
          {me && (
            <div className="cx-menu" role="menu" style={{ right: 0, top: "100%" }} onMouseLeave={() => setMe(false)}>
              <div className="cx-menu-head">{user.display_name}<small>{user.email}</small></div>
              <div className="cx-menu-theme"><span>Theme</span><ThemeSwitch /></div>
              <button role="menuitem" onClick={() => logout()}>Sign out</button>
            </div>
          )}
        </div>
      )}
    </header>
  );
}

function NavLink({ href, icon, label, count, current, sub, depth = 0, twisty }: {
  href: string; icon?: React.ReactNode; label: string; count?: number | string; current?: boolean; sub?: boolean; depth?: number; twisty?: React.ReactNode;
}) {
  return (
    <Link href={href} className={`cx-nav-item${sub ? " cx-sub" : ""}`} aria-current={current ? "page" : undefined}
      style={depth ? { paddingLeft: 16 + depth * 14 } : undefined}>
      {twisty}{icon}<span className="cx-nav-label">{label}</span>
      {count != null && count !== 0 && <span className="cx-nav-count cx-num">{count}</span>}
    </Link>
  );
}

/// Side nav keeps short: starred ones (or the first few when none is starred),
/// plus the one open now; the rest are on the All baskets page.
const NAV_MAX = 6;
function pick<T extends { id: string }>(all: T[], kind: string, openId: string, stars: string[]): T[] {
  const starred = all.filter((x) => stars.includes(`${kind}:${x.id}`));
  const base = starred.length ? starred : all.slice(0, NAV_MAX);
  const open = all.find((x) => x.id === openId);
  return open && !base.includes(open) ? [...base, open] : base;
}

function SideNav() {
  const nav = useNav();
  const { user } = useAuth();
  const pathname = usePathname() || "";
  const cur = (p: string) => pathname === p;
  const isAdmin = user?.role === "admin";
  const q = nav.quota;
  const pct = q && q.cap > 0 ? Math.min(100, Math.round((q.used / q.cap) * 100)) : 0;
  return (
    <nav className="cx-nav" aria-label="Main">
      <div className="cx-nav-scroll">
        <NavSearch />
        <NavLink href="/recent" icon={<I.recent />} label="Recent" current={cur("/recent")} />
        {nav.drive && <NavLink href={`/b/${nav.drive.id}`} icon={<I.user />} label="My Drive" current={pathname === `/b/${nav.drive.id}`} />}
        <NavLink href="/shared" icon={<I.shared />} label="Shared with me" current={cur("/shared")} />
        <NavLink href="/review" icon={<I.review />} label="Awaiting my review" current={cur("/review")} />

        <div className="cx-nav-group"><span>Baskets</span>
          {isAdmin && <Link href="/baskets?new=1" aria-label="New basket" className="cx-nav-twisty" style={{ width: 16, height: 16 }}><I.add size={16} /></Link>}
        </div>
        {pick(nav.baskets, "basket", pathname.startsWith("/b/") ? pathname.slice(3) : "", nav.stars).map((b) => (
          <NavLink key={b.id} href={`/b/${b.id}`} label={b.name} current={pathname === `/b/${b.id}`} icon={<span className="cx-bdot" style={{ background: basketColor(b.tone) }} />} />
        ))}
        <NavLink href="/baskets" icon={<I.grid />} label="All baskets" count={nav.baskets.length} current={cur("/baskets")} />

        <div className="cx-nav-group"><span>Virtual baskets</span>
          <Link href="/v/new" aria-label="New virtual basket" className="cx-nav-twisty" style={{ width: 16, height: 16 }}><I.add size={16} /></Link>
        </div>
        {nav.vbaskets.length === 0 && <div className="cx-nav-item cx-sub" style={{ cursor: "default", color: "var(--cx-text-helper)" }}>None yet</div>}
        {pick(nav.vbaskets, "vbasket", pathname.startsWith("/v/") ? pathname.slice(3) : "", nav.stars).map((v) => (
          <NavLink key={v.id} href={`/v/${v.id}`} icon={<I.vbasket />} label={v.name} sub current={pathname === `/v/${v.id}`} />
        ))}
        {nav.vbaskets.length > NAV_MAX && <NavLink href="/baskets?tab=virtual" label="All virtual baskets" count={nav.vbaskets.length} sub current={false} />}

        <div className="cx-nav-group"><span>More</span></div>
        <NavLink href="/downloads" icon={<I.download />} label="Download list" count={nav.downloads} current={cur("/downloads")} />
        <NavLink href="/trash" icon={<I.trash />} label="Trash" current={cur("/trash")} />
        {isAdmin && <NavLink href="/reports" icon={<I.rules />} label="Reports" current={cur("/reports")} />}
        <NavLink href="/docs" icon={<I.doc />} label="API docs" current={cur("/docs")} />
        {isAdmin && <NavLink href="/admin" icon={<I.settings />} label="Admin" current={pathname.startsWith("/admin")} />}
      </div>
      <div className="cx-nav-foot">
        {q && q.cap > 0 && (
          <div style={{ marginBottom: 8 }}>
            My Drive <span className="cx-num">{bytes(q.used)} of {bytes(q.cap)}</span>
            <div className={`cx-meter${pct >= 90 ? " cx-warn" : ""}`}><i style={{ width: `${pct}%` }} /></div>
          </div>
        )}
      </div>
    </nav>
  );
}

/// Search across every basket (names, fields, text inside documents).
function NavSearch() {
  const router = useRouter();
  const [q, setQ] = React.useState("");
  return (
    <form role="search" style={{ padding: "0 16px 8px" }} onSubmit={(e) => { e.preventDefault(); if (q.trim()) router.push(`/search?q=${encodeURIComponent(q.trim())}`); }}>
      <div className="cx-search-wrap">
        <I.search />
        <input className="cx-input" style={{ paddingLeft: 40, background: "var(--cx-field)" }} placeholder="Search all files" aria-label="Search all files"
          value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
    </form>
  );
}
