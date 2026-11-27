"use client";

import * as React from "react";

import { api, when, BASE_PATH, type AclResponse, type Basket, type Settings } from "@/lib/fh";
import { avatarColor, loadSettings } from "./basket-view";
import { UploadLinks } from "./upload-links";
import { Btn, DatePicker, Empty, I, Note, Pager, SearchBar, SkeletonRows, Tag, Toggle, useToast, usePageSize } from "./ui";

type PubLink = {
  id: string; token: string; file_id: string; file_name: string; expires_at: string | null; created_by: string;
  max_downloads: number | null; download_count: number; has_password: boolean; access: "view" | "download"; require_login: boolean;
};

const DAY = 86400000;
const kindOf = (p: string) => p.startsWith("user:") ? "Person" : p.startsWith("org:") ? "Agency" : p.startsWith("role:") ? "Role" : p.startsWith("iamx:") ? "IAMX role" : p === "everyone" ? "Everyone" : p;
function principalIcon(principal: string, label: string) {
  if (principal === "everyone") return <span className="cx-pr-ic" style={{ background: "var(--cx-gray-bg)", color: "var(--cx-text-primary)" }}>∗</span>;
  if (principal.startsWith("org:")) return <span className="cx-pr-ic" style={{ background: "var(--cx-brand)" }}><I.folder /></span>;
  if (principal.startsWith("role:") || principal.startsWith("iamx:")) return <span className="cx-pr-ic" style={{ background: "#393939" }}><I.user /></span>;
  return <span className="cx-pr-ic" style={{ background: avatarColor(label) }}>{label.trim().charAt(0).toUpperCase()}</span>;
}

/// Basket → Access: who can open it (who · level · expires · added by),
/// public links of its files and its upload links.
export function BasketAccess({ basket, canManage, onManage }: { basket: Basket; canManage: boolean; onManage: () => void }) {
  const toast = useToast();
  const [acl, setAcl] = React.useState<AclResponse | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [links, setLinks] = React.useState<PubLink[] | null>(null);
  const [settings, setSettings] = React.useState<Settings | null>(null);
  const load = React.useCallback(() => {
    api<AclResponse>(`/acl/basket/${basket.id}`).then(setAcl).catch((e) => { setErr((e as Error).message); setAcl({ resource_type: "basket", resource_id: basket.id, inherit: false, entries: [], inherited: [], my_level: "none" }); });
    if (canManage) api<PubLink[]>(`/baskets/${basket.id}/share-links`).then(setLinks).catch(() => setLinks([]));
    loadSettings(basket.id).then(setSettings);
  }, [basket.id, canManage]);
  React.useEffect(() => { load(); }, [load]);

  const rows = [...(acl?.entries ?? []), ...(acl?.inherited ?? [])];
  const url = (t: string) => `${window.location.origin}${BASE_PATH}/s/${t}`;
  const copy = async (t: string) => { try { await navigator.clipboard.writeText(url(t)); toast({ text: "Link copied" }); } catch { toast({ text: "Couldn't copy the link.", error: true }); } };
  const publicOn = settings?.policy.public_links !== false && !basket.vault;
  const setPublic = async (v: boolean) => {
    const s = await loadSettings(basket.id, true);
    if (!s) return;
    try {
      const out = await api<Settings>(`/baskets/${basket.id}/settings`, { method: "PUT", json: { ...s, policy: { ...s.policy, public_links: v } } });
      setSettings(out); loadSettings(basket.id, true);
      toast({ text: v ? "Public links on." : "Public links off." });
    } catch (e) { toast({ text: (e as Error).message, error: true }); }
  };

  return (
    <>
      <div className="cx-card">
        <div className="cx-card-h">
          <span><b>Who can open this basket</b><small className="cx-num">{acl ? `${rows.length} grant${rows.length === 1 ? "" : "s"}` : ""}</small></span>
          {canManage && <Btn kind="primary" md icon={<I.add />} onClick={onManage}>Add people</Btn>}
        </div>
        {err && <div style={{ padding: 16 }}><Note kind="error">{err}</Note></div>}
        <div className="cx-table-scroll">
          <table className="cx-table">
            <thead><tr><th>Who</th><th>Level</th><th className="cx-hide-sm">Expires</th><th className="cx-hide-sm">Added by</th><th style={{ width: 48 }} /></tr></thead>
            <tbody>
              {!acl && <SkeletonRows cols={5} rows={3} />}
              {rows.map((e) => {
                const exp = e.expires_at ? new Date(e.expires_at).getTime() : null;
                return (
                  <tr key={e.id}>
                    <td className="cx-name"><span className="cx-pr">{principalIcon(e.principal, e.principal_label)}
                      <span>{e.principal_label}<small>{e.inherited_from ? `From ${e.inherited_from}` : kindOf(e.principal)}</small></span></span></td>
                    <td style={{ textTransform: "capitalize", color: "var(--cx-text-primary)" }}>{e.level_name}</td>
                    <td className="cx-hide-sm cx-num">{exp ? (exp - Date.now() < 30 * DAY ? <Tag color="orange">{when(e.expires_at, false)}</Tag> : when(e.expires_at, false)) : "—"}</td>
                    <td className="cx-hide-sm">{e.granted_by_label ?? "—"}</td>
                    <td className="cx-ovf">{canManage && <Btn kind="ghost" iconOnly aria-label={`Change access for ${e.principal_label}`} icon={<I.edit />} onClick={onManage} />}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {acl && rows.length === 0 && <Empty icon={<I.user size={20} />}>No one has been given access yet</Empty>}
        </div>
      </div>
      {canManage && (
        <div className="cx-row2">
          <div className="cx-card">
            <div className="cx-card-h">
              <span><b>Public links</b><small className="cx-num">{links ? `${links.length} active` : ""}</small></span>
              {!basket.vault && <Toggle on={publicOn} onChange={setPublic} label="Allow public links" disabled={!settings} />}
            </div>
            {links == null && <div style={{ padding: 16 }}><span className="cx-skel" style={{ width: "60%" }} /></div>}
            {links?.length === 0 && <div className="cx-opt"><div className="cx-opt-tx"><span>No active links</span></div></div>}
            {links?.map((l) => (
              <div className="cx-opt" key={l.id}>
                <div className="cx-opt-tx"><b>{l.file_name}</b>
                  <span className="cx-num">{l.access === "view" ? "View only" : "Download"} · {l.require_login ? "Sign-in required" : "Anyone with the link"}{l.has_password ? " · Password" : ""} · {l.expires_at ? `Until ${when(l.expires_at, false)}` : "No expiry"} · {l.download_count} download{l.download_count === 1 ? "" : "s"}</span></div>
                <Btn kind="ghost" md onClick={() => copy(l.token)}>Copy</Btn>
                <Btn kind="danger-ghost" md onClick={async () => { try { await api(`/share-links/${l.id}`, { method: "DELETE" }); toast({ text: "Link turned off" }); load(); } catch (e) { toast({ text: (e as Error).message, error: true }); } }}>Turn off</Btn>
              </div>
            ))}
          </div>
          <div className="cx-card">
            <div className="cx-card-h"><span><b>Upload links</b></span></div>
            <div className="cx-card-b"><UploadLinks systemId={basket.id} folderId={null} placeName={basket.name} bare /></div>
          </div>
        </div>
      )}
    </>
  );
}

type Act = { id: string; actor: string; action: string; target: string | null; target_type: string | null; created_at: string };

/// Basket → Activity: who did what to which item, newest first.
export function BasketActivity({ basketId }: { basketId: string }) {
  const [rows, setRows] = React.useState<Act[] | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [page, setPage] = React.useState(0);
  const [size, setSize] = usePageSize("activity");
  const [q, setQ] = React.useState("");
  const [qDeb, setQDeb] = React.useState("");
  const [from, setFrom] = React.useState("");
  const [to, setTo] = React.useState("");
  React.useEffect(() => { const t = setTimeout(() => { setQDeb(q.trim()); setPage(0); }, 300); return () => clearTimeout(t); }, [q]);
  const params = (limit: number, offset: number) => {
    const p = new URLSearchParams({ system_id: basketId, limit: String(limit), offset: String(offset) });
    if (qDeb) p.set("q", qDeb);
    if (from) p.set("from", from);
    if (to) p.set("to", to);
    return p.toString();
  };
  React.useEffect(() => {
    setRows(null);
    // One extra row tells whether a next page exists.
    api<Act[]>(`/activity?${params(size + 1, page * size)}`)
      .then(setRows).catch((e) => { setErr((e as Error).message); setRows([]); });
  }, [basketId, page, size, qDeb, from, to]); // eslint-disable-line react-hooks/exhaustive-deps
  const shown = rows?.slice(0, size);
  const exportCsv = async () => {
    const all = await api<Act[]>(`/activity?${params(500, 0)}`);
    const esc = (v: string) => `"${(v ?? "").replace(/"/g, '""')}"`;
    const lines = ["When,Who,Action,Item", ...all.map((a) => [a.created_at, a.actor, a.action, a.target ?? ""].map(esc).join(","))];
    const url = URL.createObjectURL(new Blob(["\ufeff" + lines.join("\n")], { type: "text/csv;charset=utf-8" }));
    const el = document.createElement("a"); el.href = url; el.download = `activity-${basketId}.csv`; el.click(); URL.revokeObjectURL(url);
  };
  return (
    <div className="cx-card">
      <SearchBar value={q} onChange={setQ} placeholder="Search person, action or item">
        <DatePicker label="From" value={from} onChange={(v) => { setFrom(v); setPage(0); }} />
        <DatePicker label="To" value={to} onChange={(v) => { setTo(v); setPage(0); }} />
        <Btn kind="ghost" icon={<I.download />} onClick={exportCsv}>Export CSV</Btn>
      </SearchBar>
      {err && <div style={{ padding: 16 }}><Note kind="error">{err}</Note></div>}
      {rows == null && Array.from({ length: 5 }).map((_, i) => <div key={i} className="cx-act"><span className="cx-skel" style={{ width: `${50 + i * 8}%` }} /></div>)}
      {shown?.map((a) => (
        <div key={a.id} className="cx-act">
          <span className="cx-who"><span className="cx-who-av" style={{ background: avatarColor(a.actor) }}>{a.actor.trim().charAt(0).toUpperCase()}</span></span>
          <span>{a.actor} {a.action}{a.target ? <> <b style={{ fontWeight: 500 }}>{a.target}</b></> : null}<small className="cx-num">{when(a.created_at)}</small></span>
        </div>
      ))}
      {rows && rows.length === 0 && !err && <Empty icon={<I.recent size={20} />}>No activity yet</Empty>}
      {rows && (page > 0 || rows.length > size) && <Pager page={page} size={size} total={null} more={rows.length > size} onPage={setPage} onSize={(n) => { setSize(n); setPage(0); }} />}
    </div>
  );
}
