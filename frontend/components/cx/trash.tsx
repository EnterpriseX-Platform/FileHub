"use client";

import * as React from "react";

import { useAuth } from "@/lib/auth-context";
import { api, bytes, useApi, when, type FileRow } from "@/lib/fh";
import { useNav } from "./shell";
import { Btn, Confirm, Note, OverflowMenu, SearchBar, SkeletonRows, useFilter, useToast } from "./ui";

type Trashed = FileRow & { deleted_at?: string | null };

/// Trash — files the person may put back (their own uploads or files they can
/// edit; admins see all). Admins can also delete for good.
export function Trash() {
  const nav = useNav();
  const toast = useToast();
  const { user } = useAuth();
  const isAdmin = user?.role === "admin";
  const { data: rows, err, reload: load } = useApi<Trashed[]>("/trash");
  const [q, setQ] = React.useState("");
  const [purge, setPurge] = React.useState<Trashed | null>(null);
  // The side menu lists pinned/recent baskets only: fall back to the full list.
  const { data: all } = useApi<{ id: string; name: string }[]>("/systems");
  const basket = (id: string) => nav.baskets.find((b) => b.id === id)?.name ?? all?.find((b) => b.id === id)?.name ?? (nav.drive?.id === id ? "My Drive" : "");
  const shown = useFilter(rows, q, (f) => `${f.name} ${f.owner}`);
  const restore = async (f: Trashed) => {
    try { await api(`/files/${f.id}/restore`, { method: "POST" }); toast({ text: `${f.name} is back in ${basket(f.system_id) || "its basket"}.` }); load(); }
    catch (e) { toast({ text: (e as Error).message, error: true }); }
  };
  return (
    <div className="cx-page">
      <div className="cx-title-row"><h1 className="cx-title">Trash</h1><span className="cx-sub cx-num">{rows ? `${rows.length} file${rows.length === 1 ? "" : "s"}` : ""}</span></div>
      <SearchBar value={q} onChange={setQ} placeholder="Search in Trash" />
      {err && <Note kind="error">{err}</Note>}
      <div className="cx-table-scroll">
        <table className="cx-table">
          <thead><tr><th>Name</th><th className="cx-hide-sm">Basket</th><th className="cx-hide-sm cx-hide-md">Uploaded by</th><th className="cx-hide-sm">Deleted</th><th className="cx-right cx-hide-sm">Size</th><th /></tr></thead>
          <tbody>
            {rows == null ? <SkeletonRows cols={6} /> : shown.map((f) => (
              <tr key={f.id}>
                <td className="cx-name">{f.name}</td>
                <td className="cx-hide-sm">{basket(f.system_id)}</td>
                <td className="cx-hide-sm cx-hide-md">{f.owner}</td>
                <td className="cx-hide-sm cx-num">{when(f.deleted_at ?? f.modified_at)}</td>
                <td className="cx-right cx-num cx-hide-sm">{bytes(f.size_bytes)}</td>
                <td className="cx-right" style={{ whiteSpace: "nowrap" }}>
                  <span className="cx-hide-sm"><Btn kind="ghost" onClick={() => restore(f)}>Restore</Btn>
                  {isAdmin && <Btn kind="danger-ghost" onClick={() => setPurge(f)}>Delete forever</Btn>}</span>
                  <span className="cx-show-sm"><OverflowMenu items={[{ label: "Restore", onClick: () => restore(f) }, ...(isAdmin ? [{ label: "Delete forever", danger: true, onClick: () => setPurge(f) }] : [])]} /></span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {rows && shown.length === 0 && <div className="cx-state"><strong>{q.trim() ? "Nothing matches." : "Trash is empty."}</strong></div>}
      </div>
      {purge && <Confirm title={`Delete ${purge.name} forever?`} danger action="Delete forever"
        body="The file and all its versions are removed for good. This can't be undone."
        onClose={() => setPurge(null)} onConfirm={async () => {
          try { await api(`/files/${purge.id}?hard=true`, { method: "DELETE" }); toast({ text: `${purge.name} deleted for good.` }); }
          catch (e) { toast({ text: (e as Error).message, error: true }); }
          setPurge(null); load();
        }} />}
    </div>
  );
}
