"use client";

import * as React from "react";

/// Theme: light, dark, or follow the device ("system"). The inline script in
/// app/layout.tsx applies the `.dark` class to <html> before first paint from
/// localStorage "fh-theme"; this provider reads the same key and keeps the
/// class in step (and follows the device while the choice is "system").
export type ThemeMode = "light" | "dark" | "system";

type ThemeCtx = { mode: ThemeMode; dark: boolean; setMode: (m: ThemeMode) => void; toggle: () => void };

const Ctx = React.createContext<ThemeCtx>({ mode: "light", dark: false, setMode: () => {}, toggle: () => {} });

export function useTheme() {
  return React.useContext(Ctx);
}

const STORAGE_KEY = "fh-theme";

function readMode(): ThemeMode {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "dark" || v === "light" || v === "system") return v;
  } catch { /* storage blocked */ }
  return "light";
}

function systemDark(): boolean {
  try { return window.matchMedia("(prefers-color-scheme: dark)").matches; } catch { return false; }
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [mode, setModeState] = React.useState<ThemeMode>("light");
  const [dark, setDark] = React.useState(false);

  const apply = React.useCallback((m: ThemeMode) => {
    const d = m === "dark" || (m === "system" && systemDark());
    document.documentElement.classList.toggle("dark", d);
    setDark(d);
  }, []);

  React.useEffect(() => {
    const m = readMode();
    setModeState(m);
    apply(m);
  }, [apply]);

  // While following the device, follow its changes too.
  React.useEffect(() => {
    if (mode !== "system") return;
    let mq: MediaQueryList | null = null;
    try { mq = window.matchMedia("(prefers-color-scheme: dark)"); } catch { return; }
    const on = () => apply("system");
    mq.addEventListener("change", on);
    return () => mq?.removeEventListener("change", on);
  }, [mode, apply]);

  const setMode = React.useCallback((m: ThemeMode) => {
    try { localStorage.setItem(STORAGE_KEY, m); } catch { /* storage blocked */ }
    setModeState(m);
    apply(m);
  }, [apply]);

  const toggle = React.useCallback(() => setMode(dark ? "light" : "dark"), [dark, setMode]);

  const value = React.useMemo(() => ({ mode, dark, setMode, toggle }), [mode, dark, setMode, toggle]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/// Light / Dark / System: three small icon buttons (Carbon icons), the
/// current one filled. Label and tooltip name each choice.
const ICONS: Record<ThemeMode, React.ReactNode> = {
  light: <path d="M16 12a4 4 0 1 1-4 4 4 4 0 0 1 4-4m0-2a6 6 0 1 0 6 6 6 6 0 0 0-6-6ZM15 2h2v5h-2zM25.3 5.3l1.4 1.4-3.5 3.5-1.4-1.4zM25 15h5v2h-5zM23.2 21.8l3.5 3.5-1.4 1.4-3.5-3.5zM15 25h2v5h-2zM8.8 21.8l1.4 1.4-3.5 3.5-1.4-1.4zM2 15h5v2H2zM6.7 5.3l3.5 3.5-1.4 1.4-3.5-3.5z" />,
  dark: <path d="M13.502 5.414a15.075 15.075 0 0 0 11.594 18.194 11.113 11.113 0 0 1-7.975 3.39c-.138 0-.278.005-.418 0a11.094 11.094 0 0 1-3.2-21.584M14.98 3a1.002 1.002 0 0 0-.175.016 13.096 13.096 0 0 0 1.825 25.981c.164.006.328 0 .49 0a13.072 13.072 0 0 0 10.703-5.555 1.01 1.01 0 0 0-.783-1.565A13.08 13.08 0 0 1 15.89 4.38 1.015 1.015 0 0 0 14.98 3Z" />,
  system: <path d="M28 4H4a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h8v4H8v2h16v-2h-4v-4h8a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2ZM18 28h-4v-4h4Zm10-6H4V6h24Z" />,
};

export function ThemeSwitch() {
  const { mode, setMode } = useTheme();
  const opts: { v: ThemeMode; label: string }[] = [
    { v: "light", label: "Light" }, { v: "dark", label: "Dark" }, { v: "system", label: "Use device setting" },
  ];
  return (
    <div className="cx-theme-switch" role="radiogroup" aria-label="Theme">
      {opts.map((o) => (
        <button key={o.v} type="button" role="radio" aria-checked={mode === o.v} aria-label={o.label} title={o.label} onClick={() => setMode(o.v)}>
          <svg viewBox="0 0 32 32" width="16" height="16" aria-hidden="true" fill="currentColor">{ICONS[o.v]}</svg>
        </button>
      ))}
    </div>
  );
}

/// One icon button for sign-in pages (top-right corner, like the other products):
/// each click moves light → dark → device setting; the icon shows the current choice.
export function ThemeToggle() {
  const { mode, setMode } = useTheme();
  const next: Record<ThemeMode, ThemeMode> = { light: "dark", dark: "system", system: "light" };
  const name: Record<ThemeMode, string> = { light: "Light", dark: "Dark", system: "Use device setting" };
  return (
    <button type="button" className="cx-theme-toggle" onClick={() => setMode(next[mode])}
      aria-label={`Theme: ${name[mode]}. Switch to ${name[next[mode]]}`} title={`Theme: ${name[mode]}`}>
      <svg viewBox="0 0 32 32" width="20" height="20" aria-hidden="true" fill="currentColor">{ICONS[mode]}</svg>
    </button>
  );
}
