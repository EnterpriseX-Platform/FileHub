"use client";

import * as React from "react";

import { useAuth } from "@/lib/auth-context";
import { Empty, I } from "./ui";

/// Pages for administrators only: everyone else gets a plain refusal, not the
/// page with buttons that fail.
export function AdminOnly({ children }: { children: React.ReactNode }) {
  const { user, loading } = useAuth();
  if (loading || !user) return null;
  if (user.role !== "admin") {
    return <div className="cx-page"><Empty icon={<I.lock />}>You don&apos;t have access to this page.</Empty></div>;
  }
  return <>{children}</>;
}
