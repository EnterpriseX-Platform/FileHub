"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";

import { buildViewHref, GROUP_FIELDS, OPTIONAL_COLUMNS, SORT_FIELDS, STATUS_OPTIONS, type ViewParams } from "./view-params";

// Shared click-outside-to-close for the popovers below.
function useClickOutside(onClose: () => void, active: boolean) {
  const ref = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    if (!active) return;
    const h = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [active, onClose]);
  return ref;
}

// a11y: close an open popover on Escape, used on the menu trigger's wrapper.
function onMenuKeyDown(close: () => void) {
  return (e: React.KeyboardEvent) => {
    if (e.key === "Escape") { e.stopPropagation(); close(); }
  };
}

// a11y: shared props for a popover trigger <button> (menu pattern).
function triggerProps(open: boolean) {
  return { "aria-haspopup": "menu" as const, "aria-expanded": open };
}

function Check({ on }: { on: boolean }) {
  return <span style={{ display: "inline-block", width: 14, flexShrink: 0 }}>{on ? "✓" : ""}</span>;
}

/// "Search in this view" → full-text search via `?q=` (backend `/api/search`,
/// which indexes name/tags/owner + extracted PDF/txt content).
export function ViewSearch({ params, base = "/files" }: { params: ViewParams; base?: string }) {
  const router = useRouter();
  const [value, setValue] = React.useState(params.q ?? "");
  // Re-sync if the URL's q changes from elsewhere (e.g. the global search box).
  React.useEffect(() => { setValue(params.q ?? ""); }, [params.q]);

  const run = (next: string) => router.push(buildViewHref(base, { ...params, q: next.trim() || undefined }));

  return (
    <div className="field" style={{ height: 30, fontSize: "var(--t-sm)" }}>
      <Ico.search className="icon sm" />
      <input
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => { if (e.key === "Enter") run(value); }}
        placeholder="Search in this view…"
      />
      {value && (
        <button type="button" className="btn xs ghost icon" title="Clear search" onClick={() => { setValue(""); run(""); }}>
          <Ico.x className="icon sm" />
        </button>
      )}
    </div>
  );
}

/// Filter popover. The backend filters by `status` (also system/org/project),
/// so this drives the quick status filter; other filters come in via the
/// sidebar and show as removable pills below the toolbar.
export function FilterMenu({ params, count, base = "/files" }: { params: ViewParams; count: number; base?: string }) {
  const router = useRouter();
  const [open, setOpen] = React.useState(false);
  const ref = useClickOutside(() => setOpen(false), open);
  const go = (status: string | undefined) => { setOpen(false); router.push(buildViewHref(base, { ...params, status })); };

  return (
    <div ref={ref} style={{ position: "relative" }} onKeyDown={onMenuKeyDown(() => setOpen(false))}>
      <button type="button" className="btn sm ghost" {...triggerProps(open)} onClick={() => setOpen((v) => !v)}>
        <Ico.filter /> {count || 0} filter{count === 1 ? "" : "s"}
      </button>
      {open && (
        <div className="card" role="menu" style={{ position: "absolute", left: 0, top: "calc(100% + 6px)", zIndex: 50, width: 184, padding: 6, boxShadow: "var(--sh-popover)", display: "flex", flexDirection: "column", gap: 2 }}>
          <div className="t-xs t-subtle t-medium" style={{ padding: "4px 8px" }}>Filter by status</div>
          {STATUS_OPTIONS.map((s) => (
            <button key={s} type="button" role="menuitem" className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} onClick={() => go(s)}>
              <Check on={params.status === s} />{s}
            </button>
          ))}
          {params.status && (
            <>
              <div className="divider" style={{ margin: "4px 0" }} />
              <button type="button" role="menuitem" className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} onClick={() => go(undefined)}>Clear status</button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/// Sort popover. Applied in the server component (the backend hardcodes
/// `ORDER BY modified_at DESC`), so it sorts the fetched rows by the chosen
/// field. Clicking the active field flips direction.
export function SortMenu({ params, sort, dir, base = "/files" }: { params: ViewParams; sort: string; dir: string; base?: string }) {
  const router = useRouter();
  const [open, setOpen] = React.useState(false);
  const ref = useClickOutside(() => setOpen(false), open);
  const label = SORT_FIELDS.find((f) => f.key === sort)?.label ?? "Modified";

  const go = (key: string) => {
    const nextDir = sort === key ? (dir === "desc" ? "asc" : "desc") : "desc";
    setOpen(false);
    router.push(buildViewHref(base, {
      ...params,
      sort: key === "modified" ? undefined : key,
      dir: nextDir === "desc" ? undefined : nextDir,
    }));
  };

  return (
    <div ref={ref} style={{ position: "relative" }} onKeyDown={onMenuKeyDown(() => setOpen(false))}>
      <button type="button" className="btn sm ghost" {...triggerProps(open)} onClick={() => setOpen((v) => !v)}>
        <Ico.sort /> {label} {dir === "asc" ? "↑" : "↓"}
      </button>
      {open && (
        <div className="card" role="menu" style={{ position: "absolute", left: 0, top: "calc(100% + 6px)", zIndex: 50, width: 172, padding: 6, boxShadow: "var(--sh-popover)", display: "flex", flexDirection: "column", gap: 2 }}>
          <div className="t-xs t-subtle t-medium" style={{ padding: "4px 8px" }}>Sort by</div>
          {SORT_FIELDS.map((f) => (
            <button key={f.key} type="button" role="menuitem" className="btn xs ghost" style={{ width: "100%", justifyContent: "space-between" }} onClick={() => go(f.key)}>
              <span style={{ display: "flex", alignItems: "center" }}><Check on={sort === f.key} />{f.label}</span>
              {sort === f.key && <span className="t-subtle">{dir === "asc" ? "↑" : "↓"}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/// Group rows by a field (server-side, via `?group=`).
export function GroupMenu({ params, group, base = "/files", allowNone = true }: { params: ViewParams; group: string; base?: string; allowNone?: boolean }) {
  const router = useRouter();
  const [open, setOpen] = React.useState(false);
  const ref = useClickOutside(() => setOpen(false), open);
  // The board reuses this menu but is inherently grouped (no ungrouped mode),
  // so it passes allowNone={false} to drop the dead "No grouping" option that
  // would otherwise be a no-op (coerced back to status).
  const fields = allowNone ? GROUP_FIELDS : GROUP_FIELDS.filter((f) => f.key !== "none");
  const active = group || (allowNone ? "none" : "status");
  const label = fields.find((f) => f.key === active)?.label ?? "Status";
  const go = (key: string) => { setOpen(false); router.push(buildViewHref(base, { ...params, group: key === "none" ? undefined : key })); };

  return (
    <div ref={ref} style={{ position: "relative" }} onKeyDown={onMenuKeyDown(() => setOpen(false))}>
      <button type="button" className="btn sm ghost" {...triggerProps(open)} onClick={() => setOpen((v) => !v)}>
        <Ico.group /> {active === "none" ? "No group" : `Group: ${label}`}
      </button>
      {open && (
        <div className="card" role="menu" style={{ position: "absolute", left: 0, top: "calc(100% + 6px)", zIndex: 50, width: 172, padding: 6, boxShadow: "var(--sh-popover)", display: "flex", flexDirection: "column", gap: 2 }}>
          <div className="t-xs t-subtle t-medium" style={{ padding: "4px 8px" }}>Group by</div>
          {fields.map((f) => (
            <button key={f.key} type="button" role="menuitem" className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} onClick={() => go(f.key)}>
              <Check on={active === f.key} />{f.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/// One "View options" popover folding the rarely-used controls (grouping,
/// column visibility, save-as-view) out of the toolbar. Filter/Sort/Search
/// stay as standalone triggers — they're the everyday controls. GroupMenu /
/// PropertiesMenu remain exported for layouts that use them standalone
/// (the board's group switcher).
export function ViewOptionsMenu({ params, group, hidden, base = "/files" }: {
  params: ViewParams; group: string; hidden: string[]; base?: string;
}) {
  const router = useRouter();
  const [open, setOpen] = React.useState(false);
  const ref = useClickOutside(() => setOpen(false), open);

  const activeGroup = group || "none";
  const goGroup = (key: string) => router.push(buildViewHref(base, { ...params, group: key === "none" ? undefined : key }));

  const hiddenSet = new Set(hidden);
  const toggleCol = (key: string) => {
    const next = new Set(hiddenSet);
    next.has(key) ? next.delete(key) : next.add(key);
    const hide = OPTIONAL_COLUMNS.filter((c) => next.has(c.key)).map((c) => c.key).join(",");
    router.push(buildViewHref(base, { ...params, hide: hide || undefined }));
  };

  return (
    <div ref={ref} style={{ position: "relative" }} onKeyDown={onMenuKeyDown(() => setOpen(false))}>
      <button type="button" className="btn sm ghost" {...triggerProps(open)} onClick={() => setOpen((v) => !v)}>
        <Ico.layers /> View options
      </button>
      {open && (
        <div className="card" role="menu" style={{ position: "absolute", right: 0, top: "calc(100% + 6px)", zIndex: 50, width: 200, padding: 6, boxShadow: "var(--sh-popover)", display: "flex", flexDirection: "column", gap: 2 }}>
          <div className="t-xs t-subtle t-medium" style={{ padding: "4px 8px" }}>Group by</div>
          {GROUP_FIELDS.map((f) => (
            <button key={f.key} type="button" role="menuitemradio" aria-checked={activeGroup === f.key} className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} onClick={() => goGroup(f.key)}>
              <Check on={activeGroup === f.key} />{f.label}
            </button>
          ))}
          <div className="divider" style={{ margin: "4px 0" }} />
          <div className="t-xs t-subtle t-medium" style={{ padding: "4px 8px" }}>Columns</div>
          {OPTIONAL_COLUMNS.map((c) => (
            <button key={c.key} type="button" role="menuitemcheckbox" aria-checked={!hiddenSet.has(c.key)} className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start", alignItems: "center" }} onClick={() => toggleCol(c.key)}>
              <span className={"cb" + (!hiddenSet.has(c.key) ? " on" : "")} style={{ marginRight: 8 }} />{c.label}
            </button>
          ))}
          <div className="divider" style={{ margin: "4px 0" }} />
          <a role="menuitem" className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start" }} href={buildViewHref("/views/new", params)}>
            <Ico.pin className="icon sm" /> Save as new view
          </a>
        </div>
      )}
    </div>
  );
}

/// Column chooser (Properties). Hidden columns are carried in `?hide=`.
export function PropertiesMenu({ params, hidden, base = "/files" }: { params: ViewParams; hidden: string[]; base?: string }) {
  const router = useRouter();
  const [open, setOpen] = React.useState(false);
  const ref = useClickOutside(() => setOpen(false), open);
  const hiddenSet = new Set(hidden);
  const toggle = (key: string) => {
    const next = new Set(hiddenSet);
    next.has(key) ? next.delete(key) : next.add(key);
    const hide = OPTIONAL_COLUMNS.filter((c) => next.has(c.key)).map((c) => c.key).join(",");
    router.push(buildViewHref(base, { ...params, hide: hide || undefined }));
  };

  return (
    <div ref={ref} style={{ position: "relative" }} onKeyDown={onMenuKeyDown(() => setOpen(false))}>
      <button type="button" className="btn sm ghost" {...triggerProps(open)} onClick={() => setOpen((v) => !v)}>
        <Ico.layers /> Properties
      </button>
      {open && (
        <div className="card" role="menu" style={{ position: "absolute", left: 0, top: "calc(100% + 6px)", zIndex: 50, width: 180, padding: 6, boxShadow: "var(--sh-popover)", display: "flex", flexDirection: "column", gap: 2 }}>
          <div className="t-xs t-subtle t-medium" style={{ padding: "4px 8px" }}>Columns</div>
          {OPTIONAL_COLUMNS.map((c) => (
            <button key={c.key} type="button" role="menuitemcheckbox" aria-checked={!hiddenSet.has(c.key)} className="btn xs ghost" style={{ width: "100%", justifyContent: "flex-start", alignItems: "center" }} onClick={() => toggle(c.key)}>
              <span className={"cb" + (!hiddenSet.has(c.key) ? " on" : "")} style={{ marginRight: 8 }} />{c.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
