"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { useToast } from "@/components/toast";
import { useI18n } from "@/lib/i18n";

/// Star / unstar toggle for the friendly file view. Reads the current state on
/// mount, toggles optimistically, and reconciles with the server response.
export function StarButton({ fileId }: { fileId: string }) {
  const { t } = useI18n();
  const { toast } = useToast();
  const [starred, setStarred] = React.useState<boolean | null>(null);
  const [busy, setBusy] = React.useState(false);

  const url = `/filehub/api/files/${encodeURIComponent(fileId)}/star`;

  React.useEffect(() => {
    let alive = true;
    fetch(url, { credentials: "include", cache: "no-store" })
      .then((r) => (r.ok ? r.json() : { starred: false }))
      .then((d) => { if (alive) setStarred(!!d.starred); })
      .catch(() => { if (alive) setStarred(false); });
    return () => { alive = false; };
  }, [url]);

  const toggle = async () => {
    if (starred === null || busy) return;
    const next = !starred;
    setBusy(true);
    setStarred(next); // optimistic
    try {
      const r = await fetch(url, { method: next ? "PUT" : "DELETE", credentials: "include" });
      if (!r.ok) throw new Error(String(r.status));
      const d = await r.json();
      setStarred(!!d.starred);
      toast(t(d.starred ? "toast.starred" : "toast.unstarred"));
    } catch {
      setStarred(!next); // roll back
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      type="button"
      className={"btn sm" + (starred ? " star-on" : "")}
      onClick={toggle}
      disabled={starred === null}
      aria-pressed={!!starred}
      title={starred ? t("eday.starred") : t("eday.star")}
    >
      <Ico.star className="icon sm" /> {starred ? t("eday.starred") : t("eday.star")}
    </button>
  );
}
