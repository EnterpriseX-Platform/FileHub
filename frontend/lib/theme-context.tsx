"use client";

import * as React from "react";

/// Light/dark theme state. The inline script in app/layout.tsx applies the
/// `.dark` class to <html> BEFORE first paint (localStorage "fh-theme",
/// falling back to prefers-color-scheme), so this provider initialises from
/// the DOM — never from localStorage directly — to stay consistent with
/// whatever the script already decided.
type ThemeCtx = {
  dark: boolean;
  toggle: () => void;
};

const Ctx = React.createContext<ThemeCtx>({ dark: false, toggle: () => {} });

export function useTheme() {
  return React.useContext(Ctx);
}

const STORAGE_KEY = "fh-theme";

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  // SSR renders `false`; the first client render reads the class the inline
  // script set. The <html> class itself is correct from paint #1 either way —
  // this state only drives the toggle's label.
  const [dark, setDark] = React.useState(false);
  React.useEffect(() => {
    setDark(document.documentElement.classList.contains("dark"));
  }, []);

  const toggle = React.useCallback(() => {
    setDark((prev) => {
      const next = !prev;
      document.documentElement.classList.toggle("dark", next);
      try { localStorage.setItem(STORAGE_KEY, next ? "dark" : "light"); } catch {}
      return next;
    });
  }, []);

  const value = React.useMemo(() => ({ dark, toggle }), [dark, toggle]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
