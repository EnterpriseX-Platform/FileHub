"use client";

import { useAuth } from "@/lib/auth-context";
import { canMutate } from "@/lib/roles";

import { Ico } from "./icons";

/// Floating action button for Upload, shown only on phones (CSS `.fab` is
/// display:none above the 620px breakpoint). On desktop/tablet the topbar /
/// dashboard "Upload" button is reachable; on phones that button is cramped or
/// off-screen, so the FAB gives the primary action a permanent home. Hidden for
/// viewers (read-only) via the same role gate the backend enforces.
export function MobileUploadFab() {
  const { user } = useAuth();
  if (!canMutate(user?.role ?? null)) return null;
  return (
    <a href="/upload" className="fab" aria-label="Upload files" title="Upload files">
      <Ico.upload />
    </a>
  );
}
