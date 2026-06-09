import Link from "next/link";

/// Reusable list pager. Renders nothing for a single page.
///
/// Two modes — pass exactly one:
///   • `hrefFor(page)` → renders <Link>s (URL-driven; for server-component
///     list pages, mirrors the /files pager). `page` is 0-based.
///   • `onPage(page)` → renders <button>s (for client-state lists like the
///     members table). Use inside a client component.
///
/// No "use client" here so it stays usable from server pages; the onClick path
/// is only taken when `onPage` is supplied (i.e. already inside a client tree).
export function Pager({
  page,
  pageSize,
  total,
  hrefFor,
  onPage,
}: {
  page: number;
  pageSize: number;
  total: number;
  hrefFor?: (page: number) => string;
  onPage?: (page: number) => void;
}) {
  const pageCount = Math.max(1, Math.ceil(total / pageSize));
  if (pageCount <= 1) return null;
  const from = page * pageSize + 1;
  const to = Math.min((page + 1) * pageSize, total);

  const nav = (target: number, label: string, disabled: boolean) => {
    if (disabled) {
      return <span className="btn xs ghost" aria-disabled="true" style={{ opacity: 0.4, pointerEvents: "none" }}>{label}</span>;
    }
    if (onPage) {
      return <button type="button" className="btn xs ghost" onClick={() => onPage(target)}>{label}</button>;
    }
    return <Link className="btn xs ghost" href={hrefFor!(target)}>{label}</Link>;
  };

  return (
    <div style={{
      display: "flex", alignItems: "center", justifyContent: "space-between",
      gap: 12, padding: "12px 0", flexWrap: "wrap",
    }}>
      <span className="t-xs t-subtle">Showing {from}–{to} of {total}</span>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        {nav(page - 1, "‹ Prev", page <= 0)}
        <span className="t-xs t-subtle t-tabular">Page {page + 1} / {pageCount}</span>
        {nav(page + 1, "Next ›", page >= pageCount - 1)}
      </div>
    </div>
  );
}
