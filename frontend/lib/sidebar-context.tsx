"use client";

import { usePathname } from "next/navigation";
import * as React from "react";

/// Drawer state for the responsive sidebar. At >860px the sidebar is a normal
/// grid column and this is inert; at ≤860px the sidebar becomes an off-canvas
/// drawer (see the @media blocks in tokens.css) that the topbar hamburger
/// toggles. We auto-close on navigation and when the viewport grows back past
/// the breakpoint so the drawer never gets "stuck" open.
type SidebarCtx = {
  open: boolean;
  setOpen: (v: boolean) => void;
  toggle: () => void;
};

const Ctx = React.createContext<SidebarCtx>({
  open: false,
  setOpen: () => {},
  toggle: () => {},
});

export function useSidebar() {
  return React.useContext(Ctx);
}

export function SidebarProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = React.useState(false);
  const pathname = usePathname();

  // Close the drawer whenever the route changes (the nav links are what the
  // user just tapped).
  React.useEffect(() => {
    setOpen(false);
  }, [pathname]);

  // Close when the viewport grows past the drawer breakpoint, so a drawer left
  // open on mobile doesn't linger after a resize back to desktop.
  React.useEffect(() => {
    const mq = window.matchMedia("(min-width: 861px)");
    const onChange = () => { if (mq.matches) setOpen(false); };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  const value = React.useMemo(
    () => ({ open, setOpen, toggle: () => setOpen((v) => !v) }),
    [open],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
