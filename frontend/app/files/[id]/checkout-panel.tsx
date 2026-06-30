"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { useAuth } from "@/lib/auth-context";
import { fmtAgo } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { canMutate } from "@/lib/roles";

type Lock = {
  locked: boolean;
  by_id: string | null;
  by_name: string | null;
  at: string | null;
  by_me: boolean;
};

/// Check-out / check-in control (TOR 5.3.8.4). Shows the live lock state and,
/// for editors, a Check out / Check in button. Polls so another user's lock
/// appears without a reload.
export function CheckoutPanel({ fileId }: { fileId: string }) {
  const { user } = useAuth();
  const { t } = useI18n();
  const [lock, setLock] = React.useState<Lock | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);

  const load = React.useCallback(async () => {
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/lock`, {
        credentials: "include",
        cache: "no-store",
      });
      if (r.ok) setLock(await r.json());
    } catch { /* keep last */ }
  }, [fileId]);

  React.useEffect(() => {
    load();
    const id = setInterval(load, 12000);
    return () => clearInterval(id);
  }, [load]);

  const act = async (action: "checkout" | "checkin") => {
    setBusy(true);
    setErr(null);
    try {
      const r = await fetch(`/filehub/api/files/${encodeURIComponent(fileId)}/${action}`, {
        method: "POST",
        credentials: "include",
      });
      if (!r.ok) {
        const b = await r.json().catch(() => ({}));
        throw new Error(b.error || `failed (${r.status})`);
      }
      setLock(await r.json());
    } catch (e) {
      setErr(e instanceof Error ? e.message : "failed");
    } finally {
      setBusy(false);
    }
  };

  if (!lock) return null;
  const mutate = canMutate(user?.role ?? null);

  return (
    <section style={{ marginBottom: 12 }}>
      <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 6, display: "flex", alignItems: "center", gap: 6 }}>
        <Ico.lock className="icon sm" /> {t("co.title")}
      </div>

      {lock.locked ? (
        lock.by_me ? (
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <Pill tone="emerald" sm><span className="dot" />{t("co.byYou")}</Pill>
            <button className="btn xs" disabled={busy} onClick={() => act("checkin")}>{t("co.checkin")}</button>
          </div>
        ) : (
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <Pill tone="amber" sm><span className="dot" />{t("co.lockedBy")} {lock.by_name}</Pill>
            {lock.at && <span className="t-xs t-subtle">{fmtAgo(lock.at)}</span>}
            {user?.role === "admin" && (
              <button className="btn xs ghost" disabled={busy} onClick={() => act("checkin")}>{t("co.forceCheckin")}</button>
            )}
          </div>
        )
      ) : mutate ? (
        <button className="btn xs" disabled={busy} onClick={() => act("checkout")}>
          <Ico.download className="icon sm" /> {t("co.checkout")}
        </button>
      ) : (
        <span className="t-xs t-subtle">{t("co.available")}</span>
      )}

      {err && <div className="t-xs" style={{ color: "var(--c-rose)", marginTop: 6 }}>{err}</div>}
    </section>
  );
}
