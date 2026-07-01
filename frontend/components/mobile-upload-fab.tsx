"use client";

import { usePathname } from "next/navigation";

import { useAuth } from "@/lib/auth-context";
import { canMutate } from "@/lib/roles";

import { Ico } from "./icons";

// Everyday (consumer) routes carry their own phone bottom-nav with an Upload
// action, so the FAB would double up there. Keyed off the route (not the
// fh-view cookie) so it's correct even when a workspace-mode user visits an
// everyday page directly.
const EVERYDAY_ROUTES = new Set(["/home", "/my", "/shared", "/recent"]);

/// Floating action button for Upload, shown only on phones (CSS `.fab` is
/// display:none above the 620px breakpoint). On desktop/tablet the topbar /
/// dashboard "Upload" button is reachable; on phones that button is cramped or
/// off-screen, so the FAB gives the primary action a permanent home. Hidden for
/// viewers (read-only) via the same role gate the backend enforces.
export function MobileUploadFab() {
  const { user } = useAuth();
  const pathname = usePathname();
  if (EVERYDAY_ROUTES.has(pathname) || pathname.startsWith("/f/")) return null;
  if (!canMutate(user?.role ?? null)) return null;
  return (
    <a href="/upload" className="fab" aria-label="Upload files" title="Upload files">
      <Ico.upload />
    </a>
  );
}
