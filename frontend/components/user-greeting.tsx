"use client";

import * as React from "react";

import { useAuth } from "@/lib/auth-context";

/// Time-of-day greeting that pulls the user's display name from /api/auth/me.
/// Replaces the previous hardcoded "Good morning, Anong" so the dashboard
/// works for any signed-in user. Falls back to a neutral greeting while the
/// auth state is still loading or if the user is signed out (the layout's
/// AuthProvider keeps loading flicker brief).
export function UserGreeting() {
  const { user, loading } = useAuth();

  const hour = new Date().getHours();
  const part =
    hour < 12 ? "Good morning" :
    hour < 18 ? "Good afternoon" :
                "Good evening";

  // Just the first name keeps the heading short and matches the previous look.
  const name = user?.display_name?.split(/\s+/)[0] ?? "";

  if (loading) {
    return <h1 className="t-3xl t-semibold">{part}</h1>;
  }
  return (
    <h1 className="t-3xl t-semibold">
      {part}{name ? `, ${name}` : ""}
    </h1>
  );
}
