"use client";

import * as React from "react";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";
import { useAuth } from "@/lib/auth-context";
import type { Member } from "@/lib/api";
import { fmtAgo } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

// Mirrors backend esign.rs shapes.
type Signer = {
  id: string;
  user_id: string;
  user_name: string | null;
  seq: number;
  status: string; // pending | signed | declined
  signed_at: string | null;
};
type SignRequest = {
  id: string;
  status: string; // pending | completed | declined | expired
  order_mode: string;
  message: string | null;
  created_at: string;
  expires_at: string | null;
  signers: Signer[];
};
type SignatureMark = { id: string; label: string; kind: string; image: string; created_at: string };

/// Electronic-signature panel on the file view. Shows signing requests and
/// their signers, lets editors start a request, and lets an assigned signer
/// sign or decline (with a sequential turn guard mirrored from the backend).
export function SignPanel({ fileId, canRequest }: { fileId: string; canRequest: boolean }) {
  const { user } = useAuth();
  const { t } = useI18n();
  const [reqs, setReqs] = React.useState<SignRequest[] | null>(null);
  const [members, setMembers] = React.useState<Member[]>([]);
  const [picking, setPicking] = React.useState(false);
  const [selected, setSelected] = React.useState<string[]>([]);
  const [mode, setMode] = React.useState<"sequential" | "parallel">("sequential");
  const [busy, setBusy] = React.useState(false);
  // Signature-mark library (backend: GET/POST /api/signatures). Loaded lazily
  // the first time it's the user's turn to sign; `markId` is what gets sent
  // as signature_id.
  const [marks, setMarks] = React.useState<SignatureMark[] | null>(null);
  const [markId, setMarkId] = React.useState<string | null>(null);
  const markInputRef = React.useRef<HTMLInputElement>(null);

  const base = `/filehub/api/files/${encodeURIComponent(fileId)}/sign-requests`;

  const load = React.useCallback(async () => {
    try {
      const r = await fetch(base, { credentials: "include", cache: "no-store" });
      if (r.ok) setReqs(await r.json());
    } catch { /* keep last */ }
  }, [base]);

  React.useEffect(() => { load(); }, [load]);
  React.useEffect(() => {
    if (!picking || members.length) return;
    fetch("/filehub/api/users", { credentials: "include" })
      .then((r) => (r.ok ? r.json() : []))
      .then((m: Member[]) => setMembers(m.filter((x) => x.status === "active")))
      .catch(() => {});
  }, [picking, members.length]);

  const myTurn = (req: SignRequest, s: Signer) =>
    req.order_mode === "parallel" ||
    !req.signers.some((o) => o.status === "pending" && o.seq < s.seq);

  const act = async (reqId: string, action: "sign" | "decline") => {
    setBusy(true);
    try {
      await fetch(`/filehub/api/sign-requests/${reqId}/${action}`, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(action === "sign" && markId ? { signature_id: markId } : {}),
      });
      await load();
    } finally { setBusy(false); }
  };

  const loadMarks = React.useCallback(async () => {
    try {
      const r = await fetch("/filehub/api/signatures", { credentials: "include", cache: "no-store" });
      if (r.ok) {
        const rows: SignatureMark[] = await r.json();
        setMarks(rows);
        setMarkId((cur) => cur ?? rows[0]?.id ?? null);
      }
    } catch { /* section degrades to sign-without-mark */ }
  }, []);

  // Upload an image file as a new signature mark (stored as a data URL).
  const addMark = (file: File) => {
    const reader = new FileReader();
    reader.onload = async () => {
      setBusy(true);
      try {
        const r = await fetch("/filehub/api/signatures", {
          method: "POST",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ label: file.name.replace(/\.[a-z0-9]+$/i, ""), kind: "uploaded", image: reader.result }),
        });
        if (r.ok) {
          const created: SignatureMark = await r.json();
          setMarks((m) => [created, ...(m ?? [])]);
          setMarkId(created.id);
        }
      } finally { setBusy(false); }
    };
    reader.readAsDataURL(file);
  };

  const createRequest = async () => {
    if (!selected.length) return;
    setBusy(true);
    try {
      await fetch(base, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ order_mode: mode, signers: selected.map((uid, i) => ({ user_id: uid, seq: i })) }),
      });
      setPicking(false);
      setSelected([]);
      await load();
    } finally { setBusy(false); }
  };

  const tone = (st: string) =>
    st === "completed" || st === "signed" ? "emerald" : st === "declined" || st === "expired" ? "rose" : "amber";

  return (
    <section style={{ marginBottom: 12 }}>
      <div className="t-xs t-subtle t-medium" style={{ letterSpacing: "0.04em", textTransform: "uppercase", marginBottom: 8, display: "flex", alignItems: "center", gap: 6 }}>
        <Ico.check className="icon sm" /> {t("esign.title")}
      </div>

      {reqs === null ? (
        <div className="ai-sk" style={{ height: 14, width: "70%" }} />
      ) : reqs.length === 0 ? (
        <div className="t-xs t-subtle" style={{ marginBottom: 8 }}>{t("esign.none")}</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 8 }}>
          {reqs.map((req) => {
            const mine = req.signers.find((s) => s.user_id === user?.id && s.status === "pending");
            return (
              <div key={req.id} style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 10 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 6 }}>
                  <Pill tone={tone(req.status)} sm><span className="dot" />{t("esign.st." + req.status) || req.status}</Pill>
                  <span className="t-xs t-subtle" style={{ marginLeft: "auto" }}>{fmtAgo(req.created_at)}</span>
                </div>
                <div>
                  {/* Prototype stepper: done ✓ / current pulsing / waiting. */}
                  {req.signers.map((s, i) => {
                    const turn = req.status === "pending" && s.status === "pending" && myTurn(req, s);
                    const cls = s.status === "signed" ? "done"
                      : s.status === "declined" ? "no"
                      : turn ? "now" : "wait";
                    return (
                      <div key={s.id} className={"step " + cls}>
                        <span className="ic">
                          {s.status === "signed" ? "✓" : s.status === "declined" ? "✕" : i + 1}
                        </span>
                        <span className="tx">
                          <b className="t-trunc">{s.user_name || s.user_id}</b>
                          <span className="sub">
                            {s.status === "signed" && s.signed_at ? fmtAgo(s.signed_at)
                              : s.status === "declined" ? t("esign.st.declined")
                              : turn ? t("esign.sign") : t("esign.waiting")}
                          </span>
                        </span>
                      </div>
                    );
                  })}
                </div>
                {mine && req.status === "pending" && (
                  <>
                    {myTurn(req, mine) && (
                      <MarkPicker
                        marks={marks}
                        markId={markId}
                        onPick={setMarkId}
                        onAdd={() => markInputRef.current?.click()}
                        onFirstRender={loadMarks}
                        busy={busy}
                        t={t}
                      />
                    )}
                    <div style={{ display: "flex", gap: 6, marginTop: 8 }}>
                      <button className="btn xs primary" disabled={busy || !myTurn(req, mine)} onClick={() => act(req.id, "sign")}>
                        {myTurn(req, mine) ? t("esign.sign") : t("esign.waiting")}
                      </button>
                      <button className="btn xs ghost" disabled={busy} onClick={() => act(req.id, "decline")}>{t("esign.decline")}</button>
                    </div>
                  </>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* Hidden file input for uploading a signature-mark image. */}
      <input
        ref={markInputRef}
        type="file"
        accept="image/*"
        style={{ display: "none" }}
        aria-label={t("esign.addMark")}
        onChange={(e) => { const f = e.target.files?.[0]; if (f) addMark(f); e.target.value = ""; }}
      />

      {canRequest && !picking && (
        <button className="btn xs" onClick={() => setPicking(true)}><Ico.plus className="icon sm" /> {t("esign.request")}</button>
      )}
      {canRequest && picking && (
        <div style={{ border: "1px solid var(--border)", borderRadius: 8, padding: 10 }}>
          <div className="t-xs t-medium" style={{ marginBottom: 6 }}>{t("esign.pickSigners")}</div>
          <div style={{ maxHeight: 160, overflow: "auto", display: "flex", flexDirection: "column", gap: 2, marginBottom: 8 }}>
            {members.map((m) => (
              <label key={m.id} className="t-xs" style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer" }}>
                <input type="checkbox" checked={selected.includes(m.id)}
                  onChange={(e) => setSelected((s) => e.target.checked ? [...s, m.id] : s.filter((x) => x !== m.id))} />
                {m.display_name} <span className="t-subtle">· {m.role}</span>
              </label>
            ))}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8 }}>
            <label className="t-xs" style={{ display: "flex", gap: 4, alignItems: "center" }}>
              <input type="radio" checked={mode === "sequential"} onChange={() => setMode("sequential")} /> {t("esign.sequential")}
            </label>
            <label className="t-xs" style={{ display: "flex", gap: 4, alignItems: "center" }}>
              <input type="radio" checked={mode === "parallel"} onChange={() => setMode("parallel")} /> {t("esign.parallel")}
            </label>
          </div>
          <div style={{ display: "flex", gap: 6 }}>
            <button className="btn xs primary" disabled={busy || !selected.length} onClick={createRequest}>{t("esign.send")}</button>
            <button className="btn xs ghost" disabled={busy} onClick={() => { setPicking(false); setSelected([]); }}>{t("esign.cancel")}</button>
          </div>
        </div>
      )}
    </section>
  );
}

/// Signature-mark chooser shown when it's the user's turn: saved marks as
/// selectable thumbnails, "no mark" for a bare e-signature, and an upload
/// entry into the personal library (POST /api/signatures).
function MarkPicker({ marks, markId, onPick, onAdd, onFirstRender, busy, t }: {
  marks: SignatureMark[] | null;
  markId: string | null;
  onPick: (id: string | null) => void;
  onAdd: () => void;
  onFirstRender: () => void;
  busy: boolean;
  t: (k: string) => string;
}) {
  React.useEffect(() => { if (marks === null) onFirstRender(); }, [marks, onFirstRender]);
  return (
    <div style={{ marginTop: 8 }}>
      <div className="t-xs t-subtle" style={{ marginBottom: 4 }}>{t("esign.signAs")}</div>
      <div style={{ display: "flex", gap: 6, flexWrap: "wrap", alignItems: "center" }}>
        {(marks ?? []).map((m) => (
          <button
            key={m.id}
            type="button"
            title={m.label}
            onClick={() => onPick(m.id)}
            style={{
              padding: 2, borderRadius: 8, cursor: "pointer", background: "var(--bg)",
              border: markId === m.id ? "2px solid var(--accent)" : "1px solid var(--border)",
            }}
          >
            {/* Data-URL mark from the user's own library — plain img on purpose. */}
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img src={m.image} alt={m.label} style={{ height: 28, maxWidth: 90, display: "block", objectFit: "contain" }} />
          </button>
        ))}
        <button
          type="button"
          className={"btn xs" + (markId === null ? " primary" : " ghost")}
          onClick={() => onPick(null)}
        >
          {t("esign.noMark")}
        </button>
        <button type="button" className="btn xs ghost" disabled={busy} onClick={onAdd}>
          <Ico.plus className="icon sm" /> {t("esign.addMark")}
        </button>
      </div>
    </div>
  );
}
