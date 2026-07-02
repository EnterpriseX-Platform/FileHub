"use client";

import * as React from "react";

import { useAuth } from "@/lib/auth-context";

/// Two-persona view mode. "everyday" is the simplified, consumer-style app for
/// office staff (viewers/editors); "workspace" is the full power UI for admins.
/// The active mode is persisted in the `fh-view` cookie (read server-side in
/// app/layout.tsx). When no cookie is set yet, we default by role once auth
/// resolves: admins → workspace, everyone else → everyday.
///
/// The mode value here drives the user-menu switch and shell chrome; the actual
/// landing page is decided by routing (app/page.tsx redirects everyday users to
/// /home, and the switch navigates to the target home).
export type ViewMode = "everyday" | "workspace";

const COOKIE = "fh-view";

type Ctx = { mode: ViewMode; setMode: (m: ViewMode) => void };
const ViewCtx = React.createContext<Ctx>({ mode: "everyday", setMode: () => {} });

export const useViewMode = () => React.useContext(ViewCtx);

function writeCookie(m: ViewMode) {
  try { document.cookie = `${COOKIE}=${m};path=/;max-age=31536000;samesite=lax`; } catch {}
}

export function ViewModeProvider({
  initialMode,
  children,
}: {
  initialMode: ViewMode | null;
  children: React.ReactNode;
}) {
  const { user } = useAuth();
  const [mode, setModeState] = React.useState<ViewMode>(initialMode ?? "everyday");
  // Whether the mode was chosen explicitly (cookie present or user toggled),
  // vs. still the provisional default awaiting role.
  const [explicit, setExplicit] = React.useState(initialMode != null);

  React.useEffect(() => {
    if (explicit || !user) return;
    // Everyday is the product's face for EVERY role — admins included land on
    // the consumer shell and step into the Admin console explicitly.
    const def: ViewMode = "everyday";
    setModeState(def);
    writeCookie(def);
    setExplicit(true);
  }, [user, explicit]);

  const setMode = React.useCallback((m: ViewMode) => {
    setModeState(m);
    setExplicit(true);
    writeCookie(m);
  }, []);

  const value = React.useMemo(() => ({ mode, setMode }), [mode, setMode]);
  return <ViewCtx.Provider value={value}>{children}</ViewCtx.Provider>;
}
