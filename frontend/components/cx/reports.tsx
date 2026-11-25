"use client";

import * as React from "react";

import { api, bytes, BASE_PATH } from "@/lib/fh";
import { useNav } from "./shell";
import { Dropdown, Note, SkeletonRows, Tabs } from "./ui";

type CatRow = { category: string; file_count: number; size_bytes: number };
type TimeRow = { bucket: string; file_count: number; size_bytes: number };
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// AD-05 Reports — storage by category (TOR 4.15.15.1) and volume over time (4.15.15.2).
export function Reports() {
  const nav = useNav();
  const [tab, setTab] = React.useState<"category" | "time">("category");
  const [basket, setBasket] = React.useState("");
  const [by, setBy] = React.useState("category");
  const [bucket, setBucket] = React.useState("month");
  const [cat, setCat] = React.useState<CatRow[] | null>(null);
  const [time, setTime] = React.useState<TimeRow[] | null>(null);
  const [err, setErr] = React.useState<string | null>(null);

  React.useEffect(() => {
    setErr(null);
    const b = basket ? `&basket=${encodeURIComponent(basket)}` : "";
    if (tab === "category") { setCat(null); api<CatRow[]>(`/reports/by-category?by=${by}${b}`).then(setCat).catch((e) => setErr(e.message)); }
    else { setTime(null); api<TimeRow[]>(`/reports/by-time?bucket=${bucket}&limit=60${b}`).then(setTime).catch((e) => setErr(e.message)); }
  }, [tab, basket, by, bucket]);

  const rows = tab === "category" ? cat : time;
  const maxSize = Math.max(1, ...(rows ?? []).map((r) => r.size_bytes));
  const maxCount = Math.max(1, ...(rows ?? []).map((r) => r.file_count));
  const label = (r: CatRow | TimeRow) => {
    if ("category" in r) return r.category;
    const d = new Date(r.bucket);
    return bucket === "year" ? String(d.getUTCFullYear()) : bucket === "month" ? `${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}` : `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}`;
  };
  const totals = (rows ?? []).reduce((a, r) => ({ n: a.n + r.file_count, b: a.b + r.size_bytes }), { n: 0, b: 0 });

  return (
    <div className="cx-page" style={{ maxWidth: 1100 }}>
      <div className="cx-crumbs" />
      <div className="cx-title-row"><h1 className="cx-title">Reports</h1></div>
      <div style={{ background: "var(--cx-layer)" }}>
        <Tabs value={tab} onChange={setTab} tabs={[{ id: "category", label: "Storage by category" }, { id: "time", label: "Volume over time" }]} />
        <div className="cx-toolbar" style={{ borderBottom: "1px solid var(--cx-border-subtle)" }}>
          <Dropdown value={basket} placeholder="All baskets" renderValue={basket ? nav.baskets.find((b) => b.id === basket)?.name : undefined}
            options={[{ value: "", label: "All baskets" }, ...nav.baskets.map((b) => ({ value: b.id, label: b.name }))]} onChange={setBasket} />
          {tab === "category"
            ? <Dropdown value={by} options={[{ value: "category", label: "By category" }, { value: "format", label: "By format" }, { value: "basket", label: "By basket" }]} onChange={setBy} />
            : <Dropdown value={bucket} options={[{ value: "day", label: "By day" }, { value: "month", label: "By month" }, { value: "year", label: "By year" }]} onChange={setBucket} />}
          <span style={{ flex: 1 }} />
          {rows && <span className="cx-sub cx-num" style={{ alignSelf: "center", padding: "8px 12px", marginLeft: "auto" }}>{totals.n.toLocaleString()} files · {bytes(totals.b)}</span>}
          <a className="cx-btn cx-btn-ghost" href={`${BASE_PATH}/api/reports/export?format=xlsx&by=${by}&bucket=${bucket}${basket ? `&basket=${encodeURIComponent(basket)}` : ""}`}>Export Excel</a>
          <a className="cx-btn cx-btn-ghost" href={`${BASE_PATH}/api/reports/export?format=pdf&by=${by}&bucket=${bucket}${basket ? `&basket=${encodeURIComponent(basket)}` : ""}`}>Export PDF</a>
        </div>
        {err && <div style={{ padding: 12 }}><Note kind="error">{err}</Note></div>}
        <div className="cx-table-scroll">
          <table className="cx-table">
            <thead><tr>
              <th>{tab === "category" ? (by === "format" ? "Format" : by === "basket" ? "Basket" : "Category") : "Period"}</th>
              <th className="cx-right">Files</th><th className="cx-hide-sm" style={{ width: "30%" }} /><th className="cx-right">Storage</th><th className="cx-hide-sm" style={{ width: "30%" }} />
            </tr></thead>
            <tbody>
              {!rows && <SkeletonRows cols={5} rows={5} />}
              {rows?.map((r, i) => (
                <tr key={i}>
                  <td className="cx-name">{label(r)}</td>
                  <td className="cx-right cx-num">{r.file_count.toLocaleString("en-US")}</td>
                  <td className="cx-hide-sm"><div className="cx-meter" style={{ marginTop: 0 }}><i style={{ width: `${(r.file_count / maxCount) * 100}%` }} /></div></td>
                  <td className="cx-right cx-num">{bytes(r.size_bytes)}</td>
                  <td className="cx-hide-sm"><div className="cx-meter" style={{ marginTop: 0 }}><i style={{ width: `${(r.size_bytes / maxSize) * 100}%`, background: "var(--cx-text-secondary)" }} /></div></td>
                </tr>
              ))}
            </tbody>
          </table>
          {rows && rows.length === 0 && <div className="cx-state"><strong>No files in this range.</strong></div>}
        </div>
      </div>
    </div>
  );
}
