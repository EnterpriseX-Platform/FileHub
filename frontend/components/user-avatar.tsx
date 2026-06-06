"use client";

import * as React from "react";

import { Av, type Tone } from "@/components/primitives";
import { useAuth } from "@/lib/auth-context";

const ALLOWED: Tone[] = ["indigo", "emerald", "amber", "rose", "violet", "cyan", "fuchsia", "slate"];

function toTone(t: string | undefined): Tone | undefined {
  if (!t) return undefined;
  return (ALLOWED as readonly string[]).includes(t) ? (t as Tone) : undefined;
}

/// Topbar avatar that renders the signed-in user's initials + tone.  Replaces
/// the hardcoded `<Av name="Anong K." tone="rose" />` so every topbar shows
/// the actual user rather than a placeholder.
export function UserAvatar() {
  const { user, loading } = useAuth();
  if (loading || !user) {
    return <Av name="·" tone="slate" />;
  }
  return <Av name={user.display_name} tone={toTone(user.avatar_tone)} />;
}
