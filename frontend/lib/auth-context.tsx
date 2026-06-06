"use client";

import * as React from "react";

export type Me = {
  id: string;
  email: string;
  display_name: string;
  avatar_tone: string;
  role: string;
  status: string;
};

type AuthState = {
  user: Me | null;
  loading: boolean;
  reload: () => void;
  logout: () => Promise<void>;
};

const AuthContext = React.createContext<AuthState>({
  user: null,
  loading: true,
  reload: () => {},
  logout: async () => {},
});

export function useAuth(): AuthState {
  return React.useContext(AuthContext);
}

/// Reads the current user from /api/auth/me on mount and exposes it to the
/// rest of the app. Mutating buttons should call useAuth() first and gate
/// themselves on user != null.
export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [user, setUser]       = React.useState<Me | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [tick, setTick]       = React.useState(0);

  React.useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await fetch("/filehub/api/auth/me", { credentials: "include", cache: "no-store" });
        if (cancelled) return;
        if (r.ok) {
          setUser(await r.json());
        } else {
          setUser(null);
        }
      } catch {
        if (!cancelled) setUser(null);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [tick]);

  const reload = React.useCallback(() => setTick((n) => n + 1), []);

  const logout = React.useCallback(async () => {
    try {
      await fetch("/filehub/api/auth/logout", { method: "POST", credentials: "include" });
    } catch {}
    setUser(null);
    if (typeof window !== "undefined") {
      window.location.href = "/login";
    }
  }, []);

  return (
    <AuthContext.Provider value={{ user, loading, reload, logout }}>
      {children}
    </AuthContext.Provider>
  );
}
