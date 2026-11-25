"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";

import { useAuth } from "@/lib/auth-context";
import { api, bytes, type Basket, type Settings, type VBasket } from "@/lib/fh";
import { NewBasketModal } from "./admin";
import { loadSettings } from "./basket-view";
import { useNav } from "./shell";
import { basketColor, Btn, Dropdown, Empty, I, Tag, useFilter } from "./ui";

type Usage = { system_id: string; size_bytes: number; file_count: number };
type Kind = "" | "baskets" | "virtual" | "vault" | "encrypted" | "mounted";
const KINDS: { value: Kind; label: string }[] = [
  { value: "", label: "All types" }, { value: "baskets", label: "Baskets" }, { value: "virtual", label: "Virtual baskets" },
  { value: "vault", label: "Vault" }, { value: "encrypted", label: "Encrypted" }, { value: "mounted", label: "Mounted" },
];

/// Every basket and virtual basket the person can open, as cards (or a table);
/// the star decides what the side nav shows.
export function AllBaskets() {
  const nav = useNav();
  const router = useRouter();
  const sp = useSearchParams();
  const { user } = useAuth();
  const isAdmin = user?.role === "admin";
  const [kind, setKind] = React.useState<Kind>(sp.get("tab") === "virtual" ? "virtual" : "");
  const [view, setView] = React.useState<"cards" | "table">("cards");
  const [q, setQ] = React.useState("");
  const [usage, setUsage] = React.useState<Map<string, Usage> | null>(null);
  const [enc, setEnc] = React.useState<Set<string>>(new Set());
  const creating = sp.get("new") === "1";

  React.useEffect(() => { try { const v = localStorage.getItem("fh.baskets.view"); if (v === "table" || v === "cards") setView(v); } catch { /* storage off */ } }, []);
  const pickView = (v: "cards" | "table") => { setView(v); try { localStorage.setItem("fh.baskets.view", v); } catch { /* storage off */ } };
  React.useEffect(() => {
    api<{ storage_by_system: Usage[] }>("/stats").then((s) => setUsage(new Map(s.storage_by_system.map((x) => [x.system_id, x])))).catch(() => setUsage(new Map()));
  }, [nav.baskets]);
  React.useEffect(() => {
    let off = false;
    Promise.all(nav.baskets.map((b) => loadSettings(b.id).then((s: Settings | null) => (s?.policy.encrypt ? b.id : null)))).then((ids) => {
      if (!off) setEnc(new Set(ids.filter((x): x is string => !!x)));
    });
    return () => { off = true; };
  }, [nav.baskets]);

  const starred = (k: string, id: string) => nav.stars.includes(`${k}:${id}`);
  const Star = ({ k, id, name }: { k: "basket" | "vbasket"; id: string; name: string }) => {
    const on = starred(k, id);
    return (
      <button className="cx-star" aria-pressed={on} aria-label={on ? `Remove star from ${name}` : `Star ${name}`}
        onClick={(e) => { e.preventDefault(); e.stopPropagation(); nav.toggleStar(k, id); }}>
        {on ? <I.starFilled /> : <I.star />}
      </button>
    );
  };
  const isEnc = (b: Basket) => b.vault || enc.has(b.id);
  const baskets = [...useFilter(nav.baskets, q, (b) => `${b.name} ${b.bucket} ${b.description ?? ""}`)]
    .filter((b) => kind === "" || kind === "baskets" || (kind === "vault" && b.vault) || (kind === "encrypted" && isEnc(b)) || (kind === "mounted" && !!b.mount_path))
    .sort((a, b) => Number(starred("basket", b.id)) - Number(starred("basket", a.id)) || a.name.localeCompare(b.name));
  const vMatch = useFilter(nav.vbaskets, q, (v) => v.name);
  const vbs = (kind === "" || kind === "virtual" ? [...vMatch] : [])
    .sort((a, b) => Number(starred("vbasket", b.id)) - Number(starred("vbasket", a.id)) || a.name.localeCompare(b.name));
  const showB = kind !== "virtual";
  const totFiles = usage ? nav.baskets.reduce((n, b) => n + (usage.get(b.id)?.file_count ?? 0), 0) : null;
  const totSize = usage ? nav.baskets.reduce((n, b) => n + (usage.get(b.id)?.size_bytes ?? 0), 0) : null;

  const tags = (b: Basket) => <>
    {isEnc(b) && <Tag color="purple">Encrypted</Tag>}
    {b.vault && <Tag color="red">Vault</Tag>}
    {b.mount_path && <Tag color="gray">Mounted</Tag>}
    {b.status !== "live" && <Tag color="gray">Paused</Tag>}
  </>;
  const meter = (b: Basket) => {
    const used = usage?.get(b.id)?.size_bytes ?? 0;
    const pct = b.quota_bytes > 0 ? Math.min(100, Math.round((used / b.quota_bytes) * 100)) : 0;
    return { used, pct };
  };

  return (
    <div className="cx-page">
      <div className="cx-ph" style={{ marginTop: 0 }}>
        <div><h1 className="cx-title">Baskets</h1>
          <div className="cx-sub cx-num">{nav.baskets.length} basket{nav.baskets.length === 1 ? "" : "s"}{nav.vbaskets.length ? ` · ${nav.vbaskets.length} virtual` : ""}{totFiles != null ? ` · ${totFiles} files · ${bytes(totSize)}` : ""}</div></div>
        <div className="cx-acts">
          <Link className="cx-btn cx-btn-tertiary cx-btn-md cx-hide-sm" href="/v/new">Virtual basket<I.add /></Link>
          {isAdmin && <Btn kind="primary" md icon={<I.add />} onClick={() => router.push("/baskets?new=1")}>New basket</Btn>}
        </div>
      </div>
      <div className="cx-toolbar cx-framed">
        <div className="cx-grow cx-search-wrap"><I.search />
          <input className="cx-search" placeholder="Search baskets" aria-label="Search baskets" value={q} onChange={(e) => setQ(e.target.value)} /></div>
        <Dropdown className="cx-hide-sm" value={kind} options={KINDS} onChange={(v) => setKind(v as Kind)} />
        <div className="cx-viewtoggle" role="group" aria-label="View">
          <button aria-pressed={view === "cards"} aria-label="Cards" onClick={() => pickView("cards")}><I.grid /></button>
          <button aria-pressed={view === "table"} aria-label="Table" onClick={() => pickView("table")}><I.list /></button>
        </div>
      </div>

      {view === "cards" ? (
        (showB ? baskets.length : 0) + vbs.length > 0 && (
          <div className="cx-bgrid">
            {showB && baskets.map((b) => {
              const u = usage?.get(b.id);
              const { used, pct } = meter(b);
              return (
                <Link key={b.id} href={`/b/${b.id}`} className="cx-bcard">
                  <Star k="basket" id={b.id} name={b.name} />
                  <div className="cx-bcard-t"><span className="cx-bicon" style={{ background: basketColor(b.tone) }}><I.folder size={20} /></span>
                    <span style={{ minWidth: 0, paddingRight: 32 }}><b>{b.name}</b><small>{b.mount_path ? `mount:${b.mount_path}` : b.bucket}</small></span></div>
                  <div className="cx-bcard-st">
                    <span><b>{u ? u.file_count : "–"}</b>files</span>
                    <span><b>{bytes(used) || "0 B"}</b>{b.quota_bytes > 0 ? `of ${bytes(b.quota_bytes)}` : "no limit"}</span>
                  </div>
                  <div className={`cx-meter${pct >= 90 ? " cx-warn" : ""}`}><i style={{ width: `${pct}%` }} /></div>
                  <div className="cx-bcard-tags">{tags(b)}{pct >= 90 && <Tag color="orange">Almost full</Tag>}</div>
                </Link>
              );
            })}
            {vbs.map((v: VBasket) => (
              <Link key={v.id} href={`/v/${v.id}`} className="cx-bcard">
                <Star k="vbasket" id={v.id} name={v.name} />
                <div className="cx-bcard-t"><span className="cx-bicon cx-virtual" style={{ background: "#6929c4" }}><I.filter size={20} /></span>
                  <span style={{ minWidth: 0, paddingRight: 32 }}><b>{v.name}</b><small>virtual</small></span></div>
                <div className="cx-bcard-st"><span><b>{v.all_sources ? "All" : v.sources.length}</b>source baskets</span></div>
                <div className="cx-bcard-tags"><Tag color="purple">Virtual</Tag>{v.personal && <Tag color="gray">Personal</Tag>}</div>
              </Link>
            ))}
          </div>
        )
      ) : (
        <div className="cx-table-scroll" style={{ border: "1px solid var(--cx-border-subtle)" }}>
          <table className="cx-table">
            <thead><tr><th style={{ width: 48 }} /><th>Basket</th><th className="cx-right">Files</th><th className="cx-right cx-hide-sm">Size</th><th className="cx-right cx-hide-sm">Limit</th><th className="cx-hide-sm">Type</th></tr></thead>
            <tbody>
              {showB && baskets.map((b) => {
                const { used, pct } = meter(b);
                return (
                  <tr key={b.id} onDoubleClick={() => router.push(`/b/${b.id}`)}>
                    <td><Star k="basket" id={b.id} name={b.name} /></td>
                    <td className="cx-name"><Link className="cx-fname" href={`/b/${b.id}`}><span className="cx-bdot" style={{ background: basketColor(b.tone) }} />
                      <span><span className="cx-fname-t">{b.name}</span><span className="cx-fname-meta cx-mono">{b.bucket}</span></span></Link></td>
                    <td className="cx-right cx-num">{usage?.get(b.id)?.file_count ?? ""}</td>
                    <td className="cx-right cx-num cx-hide-sm" style={pct >= 90 ? { color: "var(--cx-warn)" } : undefined}>{bytes(used)}</td>
                    <td className="cx-right cx-num cx-hide-sm">{b.quota_bytes ? bytes(b.quota_bytes) : "—"}</td>
                    <td className="cx-hide-sm"><span className="cx-bcard-tags">{tags(b)}</span></td>
                  </tr>
                );
              })}
              {vbs.map((v) => (
                <tr key={v.id} onDoubleClick={() => router.push(`/v/${v.id}`)}>
                  <td><Star k="vbasket" id={v.id} name={v.name} /></td>
                  <td className="cx-name"><Link className="cx-fname" href={`/v/${v.id}`}><I.filter className="cx-folder-ic" /><span><span className="cx-fname-t">{v.name}</span></span></Link></td>
                  <td /><td className="cx-hide-sm" /><td className="cx-hide-sm" />
                  <td className="cx-hide-sm"><Tag color="purple">Virtual</Tag></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {(showB ? baskets.length : 0) + vbs.length === 0 && <Empty>{q.trim() || kind ? "No baskets match" : "No baskets yet"}</Empty>}
      {creating && isAdmin && <NewBasketModal onClose={() => router.replace("/baskets")} onCreated={(id) => { nav.reload(); router.push(`/b/${id}`); }} />}
    </div>
  );
}
