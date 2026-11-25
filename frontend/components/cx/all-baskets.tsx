"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";

import { bytes } from "@/lib/fh";
import { useNav } from "./shell";
import { I, SearchBar, Tabs, Tag, useFilter } from "./ui";

/// Every basket and virtual basket the person can open; the star decides what
/// the side nav shows.
export function AllBaskets() {
  const nav = useNav();
  const router = useRouter();
  const sp = useSearchParams();
  const tab = sp.get("tab") === "virtual" ? "virtual" : "baskets";
  const [q, setQ] = React.useState("");
  const starred = (kind: string, id: string) => nav.stars.includes(`${kind}:${id}`);
  const Star = ({ kind, id, name }: { kind: "basket" | "vbasket"; id: string; name: string }) => {
    const on = starred(kind, id);
    return (
      <button className="cx-star" aria-pressed={on} aria-label={on ? `Remove star from ${name}` : `Star ${name}`}
        title={on ? "Starred — shown in the side menu" : "Star to show in the side menu"} onClick={() => nav.toggleStar(kind, id)}>
        {on ? <I.starFilled /> : <I.star />}
      </button>
    );
  };
  const baskets = [...useFilter(nav.baskets, q, (b) => `${b.name} ${b.bucket} ${b.description ?? ""}`)]
    .sort((a, b) => Number(starred("basket", b.id)) - Number(starred("basket", a.id)) || a.name.localeCompare(b.name));
  const vbs = [...useFilter(nav.vbaskets, q, (v) => v.name)]
    .sort((a, b) => Number(starred("vbasket", b.id)) - Number(starred("vbasket", a.id)) || a.name.localeCompare(b.name));

  return (
    <div className="cx-page" style={{ maxWidth: 1000 }}>
      <div className="cx-title-row"><h1 className="cx-title">All baskets</h1></div>
      <Tabs tabs={[{ id: "baskets", label: `Baskets (${nav.baskets.length})` }, { id: "virtual", label: `Virtual baskets (${nav.vbaskets.length})` }]}
        value={tab} onChange={(t) => router.replace(t === "virtual" ? "/baskets?tab=virtual" : "/baskets")} />
      <SearchBar value={q} onChange={setQ} placeholder="Search by name" />
      <div className="cx-table-scroll">
        <table className="cx-table">
          {tab === "baskets" ? (<>
            <thead><tr><th style={{ width: 40 }} /><th>Basket</th><th className="cx-hide-sm">About</th><th className="cx-right cx-hide-sm">Limit</th></tr></thead>
            <tbody>
              {baskets.map((b) => (
                <tr key={b.id}>
                  <td><Star kind="basket" id={b.id} name={b.name} /></td>
                  <td className="cx-name"><Link className="cx-filelink" href={`/b/${b.id}`}>{b.vault ? <I.lock /> : <I.basket />}<span>{b.name}</span></Link>{b.vault && <> <Tag color="blue">Vault</Tag></>}</td>
                  <td className="cx-hide-sm" style={{ whiteSpace: "normal" }}>{b.description}</td>
                  <td className="cx-right cx-num cx-hide-sm">{b.quota_bytes ? bytes(b.quota_bytes) : ""}</td>
                </tr>
              ))}
            </tbody>
          </>) : (<>
            <thead><tr><th style={{ width: 40 }} /><th>Virtual basket</th></tr></thead>
            <tbody>
              {vbs.map((v) => (
                <tr key={v.id}>
                  <td><Star kind="vbasket" id={v.id} name={v.name} /></td>
                  <td className="cx-name"><Link className="cx-filelink" href={`/v/${v.id}`}><I.vbasket /><span>{v.name}</span></Link></td>
                </tr>
              ))}
            </tbody>
          </>)}
        </table>
        {((tab === "baskets" ? baskets : vbs).length === 0) && <div className="cx-state"><strong>{q.trim() ? "Nothing matches." : "None yet."}</strong></div>}
      </div>
    </div>
  );
}
