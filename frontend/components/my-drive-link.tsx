"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { SideRow } from "@/components/primitives";
import { useAuth } from "@/lib/auth-context";

/// Client component shown in the sidebar that lazy-loads the caller's
/// personal drive via /api/personal-drive and renders a link into the files
/// page filtered to that system.  Hidden while auth loads or for guests.
export function MyDriveLink() {
  const { user, loading } = useAuth();
  const [driveId, setDriveId] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!user) { setDriveId(null); return; }
    let cancelled = false;
    (async () => {
      try {
        const r = await fetch("/filehub/api/personal-drive", { credentials: "include", cache: "no-store" });
        if (!cancelled && r.ok) {
          const sys = await r.json();
          setDriveId(sys.id);
        }
      } catch { /* hidden silently — no harm if it fails */ }
    })();
    return () => { cancelled = true; };
  }, [user]);

  if (loading || !user || !driveId) return null;
  return (
    <SideRow
      href={`/files?system_id=${encodeURIComponent(driveId)}`}
      icon={<Ico.home className="icon sm" />}
      label="My Drive"
    />
  );
}
