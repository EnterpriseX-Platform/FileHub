"use client";

import * as React from "react";

import type { StatusDef, TagColor } from "@/lib/fh";

// ── Icons (Carbon-style 16px strokes) ─────────────────────────────────────
type IP = { size?: number; className?: string };
const svg = (d: React.ReactNode) => function Icon({ size = 16, className }: IP) {
  return <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.2" aria-hidden className={className}>{d}</svg>;
};
export const I = {
  chevronDown: svg(<path d="M4 6l4 4 4-4" />),
  chevronRight: svg(<path d="M6 4l4 4-4 4" />),
  close: svg(<path d="M4 4l8 8M12 4l-8 8" />),
  add: svg(<path d="M8 3v10M3 8h10" />),
  search: svg(<><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5L14 14" /></>),
  menu: svg(<path d="M2 4h12M2 8h12M2 12h12" />),
  bell: svg(<><path d="M4 11V7a4 4 0 0 1 8 0v4l1 1.5H3z" /><path d="M6.5 14a1.5 1.5 0 0 0 3 0" /></>),
  hub: svg(<><rect x="2" y="2" width="5" height="5" /><rect x="9" y="2" width="5" height="5" /><rect x="2" y="9" width="5" height="5" /><path d="M9 11.5h5M11.5 9v5" /></>),
  folder: svg(<path d="M1.5 3.5h5l1.5 1.5h6.5v8h-13z" />),
  vfolder: svg(<path d="M1.5 3.5h5l1.5 1.5h6.5v8h-13z" strokeDasharray="2 1.5" />),
  basket: svg(<><path d="M2 6h12l-1.2 7.5H3.2z" /><path d="M5 6l3-4 3 4" /></>),
  vbasket: svg(<><path d="M2 6h12l-1.2 7.5H3.2z" strokeDasharray="2 1.5" /><path d="M5 6l3-4 3 4" /></>),
  upload: svg(<><path d="M8 11V3M4.5 6.5L8 3l3.5 3.5" /><path d="M2.5 13.5h11" /></>),
  download: svg(<><path d="M8 3v8M4.5 7.5L8 11l3.5-3.5" /><path d="M2.5 13.5h11" /></>),
  overflow: svg(<><circle cx="8" cy="3.5" r=".8" fill="currentColor" /><circle cx="8" cy="8" r=".8" fill="currentColor" /><circle cx="8" cy="12.5" r=".8" fill="currentColor" /></>),
  edit: svg(<path d="M3 11.5V13h1.5l7-7L10 4.5l-7 7zM9.5 5L11 6.5" />),
  trash: svg(<><path d="M3 4.5h10M6 4.5V3h4v1.5M4.5 4.5l.7 9h5.6l.7-9" /></>),
  lock: svg(<><rect x="3.5" y="7" width="9" height="6.5" /><path d="M5.5 7V5a2.5 2.5 0 015 0v2" /></>),
  user: svg(<><circle cx="8" cy="5.5" r="2.5" /><path d="M3 14c.5-3 2.5-4.5 5-4.5s4.5 1.5 5 4.5" /></>),
  recent: svg(<><circle cx="8" cy="8" r="6" /><path d="M8 4.5V8l2.5 1.5" /></>),
  shared: svg(<><circle cx="5" cy="6" r="2" /><circle cx="11" cy="6" r="2" /><path d="M1.5 13c.3-2 1.6-3 3.5-3s3.2 1 3.5 3M7.5 13c.3-2 1.6-3 3.5-3s3.2 1 3.5 3" /></>),
  review: svg(<><path d="M3 2.5h10v11H3z" /><path d="M5.5 8l2 2 3-3.5" /></>),
  settings: svg(<><circle cx="8" cy="8" r="2" /><path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" /></>),
  link: svg(<path d="M6.5 9.5l3-3M7 4.5l1-1a2.5 2.5 0 013.5 3.5l-1 1M9 11.5l-1 1A2.5 2.5 0 014.5 9l1-1" />),
  arrowUp: svg(<path d="M8 13V3M4 7l4-4 4 4" />),
  arrowDown: svg(<path d="M8 3v10M4 9l4 4 4-4" />),
  sort: svg(<path d="M5 3v10M3 11l2 2 2-2M11 13V3M9 5l2-2 2 2" />),
  warning: svg(<><path d="M8 2l6.5 12h-13z" /><path d="M8 6.5v3.5M8 11.5v.5" /></>),
  check: svg(<path d="M3 8.5l3 3 7-7" />),
  move: svg(<path d="M2 8h12M10 4l4 4-4 4" />),
  rules: svg(<path d="M2 4h7M2 8h12M2 12h5M11 2v4M13 10v4" />),
  doc: svg(<><path d="M3.5 1.5h6l3 3v10h-9z" /><path d="M9.5 1.5v3h3M5.5 8h5M5.5 10.5h5" /></>),
  star: svg(<path d="M8 1.8l1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6l-3.8 2 .7-4.3-3.1-3 4.3-.6z" />),
  starFilled: svg(<path d="M8 1.8l1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6l-3.8 2 .7-4.3-3.1-3 4.3-.6z" fill="currentColor" />),
  copy: svg(<><rect x="5" y="5" width="8.5" height="8.5" /><path d="M3 10.5V2.5h8" /></>),
};

// ── Button ────────────────────────────────────────────────────────────────
type BtnKind = "primary" | "secondary" | "tertiary" | "ghost" | "danger" | "danger-ghost";
export function Btn({ kind = "secondary", icon, iconOnly, children, className = "", ...p }:
  React.ButtonHTMLAttributes<HTMLButtonElement> & { kind?: BtnKind; icon?: React.ReactNode; iconOnly?: boolean }) {
  return (
    <button type="button" {...p} className={`cx-btn cx-btn-${kind}${iconOnly ? " cx-btn-icon" : ""} ${className}`}>
      {children}{icon}
    </button>
  );
}

// ── Tag ───────────────────────────────────────────────────────────────────
export function Tag({ color = "gray", children, onRemove, title }: { color?: TagColor; children: React.ReactNode; onRemove?: () => void; title?: string }) {
  return (
    <span className={`cx-tag cx-tag-${color}`} title={title}>
      {children}
      {onRemove && <button aria-label="Remove" onClick={onRemove}><I.close size={10} /></button>}
    </span>
  );
}
export function StatusTag({ code, statuses }: { code: string; statuses?: StatusDef[] }) {
  const s = statuses?.find((x) => x.code === code);
  return <Tag color={s?.color ?? "gray"}>{s?.label ?? code}</Tag>;
}

// ── Text input ────────────────────────────────────────────────────────────
export function TextField({ label, required, helper, error, id, className, ...p }:
  React.InputHTMLAttributes<HTMLInputElement> & { label?: string; helper?: string; error?: string | null }) {
  const auto = React.useId();
  const fid = id ?? auto;
  return (
    <div className={`cx-field${className ? ` ${className}` : ""}`}>
      {label && <label className="cx-label" htmlFor={fid}>{label}{required && <span className="cx-req">*</span>}</label>}
      <input id={fid} className="cx-input" aria-invalid={error ? true : undefined} {...p} />
      {error ? <span className="cx-error">{error}</span> : helper ? <span className="cx-helper">{helper}</span> : null}
    </div>
  );
}
export function TextArea({ label, id, ...p }: React.TextareaHTMLAttributes<HTMLTextAreaElement> & { label?: string }) {
  const auto = React.useId();
  const fid = id ?? auto;
  return (
    <div className="cx-field">
      {label && <label className="cx-label" htmlFor={fid}>{label}</label>}
      <textarea id={fid} className="cx-textarea" {...p} />
    </div>
  );
}
export function Check({ label, checked, onChange, id }: { label: React.ReactNode; checked: boolean; onChange: (v: boolean) => void; id?: string }) {
  return (
    <label className="cx-checkbox">
      <input id={id} type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span>{label}</span>
    </label>
  );
}

// ── Dropdown (system dropdown, never a native <select>) ───────────────────
export type Opt = { value: string; label: string; hint?: string };
export function Dropdown({ label, value, options, onChange, placeholder = "Choose", required, searchable, id, className, triggerClassName, disabled, multi, values, onChangeMulti, renderValue, up }: {
  label?: string; value?: string | null; options: Opt[]; onChange?: (v: string) => void; placeholder?: string; required?: boolean;
  searchable?: boolean; id?: string; className?: string; triggerClassName?: string; disabled?: boolean;
  multi?: boolean; values?: string[]; onChangeMulti?: (v: string[]) => void; renderValue?: string; up?: boolean;
}) {
  const [open, setOpen] = React.useState(false);
  const [q, setQ] = React.useState("");
  const [active, setActive] = React.useState(0);
  const ref = React.useRef<HTMLDivElement>(null);
  const auto = React.useId();
  const fid = id ?? auto;
  React.useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);
  const showSearch = searchable ?? options.length > 10;
  const filtered = q ? options.filter((o) => (o.label + " " + (o.hint ?? "")).toLowerCase().includes(q.toLowerCase())) : options;
  const sel = multi ? options.filter((o) => values?.includes(o.value)) : options.find((o) => o.value === value);
  const shown = renderValue ?? (multi ? (sel as Opt[]).map((o) => o.label).join(", ") : (sel as Opt | undefined)?.label);
  const pick = (o: Opt) => {
    if (multi) {
      const cur = values ?? [];
      onChangeMulti?.(cur.includes(o.value) ? cur.filter((v) => v !== o.value) : [...cur, o.value]);
    } else {
      onChange?.(o.value);
      setOpen(false);
    }
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") { setOpen(false); return; }
    if (e.key === "ArrowDown") { e.preventDefault(); if (!open) setOpen(true); setActive((a) => Math.min(a + 1, filtered.length - 1)); }
    if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => Math.max(a - 1, 0)); }
    if (e.key === "Enter" && open && filtered[active]) { e.preventDefault(); pick(filtered[active]); }
  };
  return (
    <div className={`cx-field cx-dd ${className ?? ""}`} ref={ref} onKeyDown={onKey}>
      {label && <label className="cx-label" htmlFor={fid}>{label}{required && <span className="cx-req">*</span>}</label>}
      <button id={fid} type="button" className={`cx-dd-trigger ${triggerClassName ?? ""}`} aria-haspopup="listbox" aria-expanded={open}
        disabled={disabled} onClick={() => { setOpen((o) => !o); setQ(""); setActive(0); }}>
        {shown ? <span className="cx-dd-val">{shown}</span> : <span className="cx-dd-ph">{placeholder}</span>}
        <I.chevronDown />
      </button>
      {open && (
        <div className={`cx-dd-menu${up ? " cx-up" : ""}`} role="listbox" aria-multiselectable={multi || undefined}>
          {showSearch && (
            <div className="cx-dd-search"><input autoFocus placeholder="Filter" value={q} onChange={(e) => { setQ(e.target.value); setActive(0); }} aria-label="Filter options" /></div>
          )}
          {filtered.length === 0 && <div className="cx-dd-empty">No matches</div>}
          {filtered.map((o, i) => {
            const selected = multi ? values?.includes(o.value) : o.value === value;
            return (
              <button key={o.value} type="button" role="option" aria-selected={selected} data-active={i === active}
                className="cx-dd-opt" onMouseEnter={() => setActive(i)} onClick={() => pick(o)}>
                {multi && <input type="checkbox" readOnly checked={!!selected} tabIndex={-1} style={{ accentColor: "var(--cx-text-primary)" }} />}
                <span>{o.label}</span>{o.hint && <small>{o.hint}</small>}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ── Modal ─────────────────────────────────────────────────────────────────
export function Modal({ title, label, onClose, children, footer, size, initialFocus }: {
  title: React.ReactNode; label?: string; onClose: () => void; children: React.ReactNode; footer?: React.ReactNode;
  size?: "narrow" | "wide"; initialFocus?: boolean;
}) {
  const ref = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", h);
    if (initialFocus !== false) {
      const first = ref.current?.querySelector<HTMLElement>("input, textarea, .cx-modal-body button");
      first?.focus();
    }
    return () => { document.removeEventListener("keydown", h); prev?.focus?.(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <div className="cx-overlay" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className={`cx-modal${size ? ` cx-${size}` : ""}`} role="dialog" aria-modal="true" ref={ref}>
        <div className="cx-modal-head">
          <div>{label && <div className="cx-modal-label">{label}</div>}<h2>{title}</h2></div>
          <button className="cx-close" aria-label="Close" onClick={onClose}><I.close /></button>
        </div>
        <div className="cx-modal-body">{children}</div>
        {footer && <div className="cx-modal-foot">{footer}</div>}
      </div>
    </div>
  );
}

/// In-page confirmation (the viewer frame blocks window.confirm).
export function Confirm({ title, body, action, danger, onConfirm, onClose }: {
  title: string; body: React.ReactNode; action: string; danger?: boolean; onConfirm: () => Promise<void> | void; onClose: () => void;
}) {
  const [busy, setBusy] = React.useState(false);
  const [err, setErr] = React.useState<string | null>(null);
  return (
    <Modal title={title} size="narrow" onClose={onClose} footer={<>
      <Btn kind="secondary" onClick={onClose}>Cancel</Btn>
      <Btn kind={danger ? "danger" : "primary"} disabled={busy} onClick={async () => {
        setBusy(true); setErr(null);
        try { await onConfirm(); onClose(); } catch (e) { setErr((e as Error).message); setBusy(false); }
      }}>{busy ? `${action}…` : action}</Btn>
    </>}>
      <div>{body}</div>
      {err && <Note kind="error">{err}</Note>}
    </Modal>
  );
}

// ── Notification ──────────────────────────────────────────────────────────
export function Note({ kind = "info", children }: { kind?: "error" | "success" | "info"; children: React.ReactNode }) {
  return <div className={`cx-note cx-note-${kind}`} role={kind === "error" ? "alert" : "status"}>{children}</div>;
}

type Toast = { id: number; text: string; error?: boolean; undo?: () => void };
const ToastCtx = React.createContext<(t: Omit<Toast, "id">) => void>(() => {});
export function useToast() { return React.useContext(ToastCtx); }
export function ToastHost({ children }: { children: React.ReactNode }) {
  const [list, setList] = React.useState<Toast[]>([]);
  const push = React.useCallback((t: Omit<Toast, "id">) => {
    const id = Date.now() + Math.random();
    setList((l) => [...l, { ...t, id }]);
    setTimeout(() => setList((l) => l.filter((x) => x.id !== id)), t.undo ? 6000 : 4000);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="cx-toasts" aria-live="polite">
        {list.map((t) => (
          <div key={t.id} className={`cx-toast${t.error ? " cx-toast-error" : ""}`}>
            <span>{t.text}</span>
            {t.undo && <button onClick={() => { t.undo?.(); setList((l) => l.filter((x) => x.id !== t.id)); }}>Undo</button>}
            <button aria-label="Dismiss" onClick={() => setList((l) => l.filter((x) => x.id !== t.id))}><I.close size={12} /></button>
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

// ── Tabs ──────────────────────────────────────────────────────────────────
export function Tabs<T extends string>({ tabs, value, onChange }: { tabs: { id: T; label: string }[]; value: T; onChange: (t: T) => void }) {
  return (
    <div className="cx-tabs" role="tablist">
      {tabs.map((t) => (
        <button key={t.id} role="tab" aria-selected={t.id === value} className="cx-tab" onClick={() => onChange(t.id)}>{t.label}</button>
      ))}
    </div>
  );
}

// ── Skeleton rows (loading state — no spinners) ───────────────────────────
export function SkeletonRows({ cols, rows = 6 }: { cols: number; rows?: number }) {
  return (
    <>
      {Array.from({ length: rows }).map((_, r) => (
        <tr key={r} className="cx-skel-row" aria-hidden>
          {Array.from({ length: cols }).map((__, c) => (
            <td key={c}><span className="cx-skel" style={{ width: c === 0 ? "16px" : c === 1 ? `${55 + ((r * 17) % 35)}%` : "60%" }} /></td>
          ))}
        </tr>
      ))}
    </>
  );
}

export function useOutside<T extends HTMLElement>(open: boolean, close: () => void) {
  const ref = React.useRef<T>(null);
  React.useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) close(); };
    const k = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
    document.addEventListener("mousedown", h);
    document.addEventListener("keydown", k);
    return () => { document.removeEventListener("mousedown", h); document.removeEventListener("keydown", k); };
  }, [open, close]);
  return ref;
}

/// Small overflow menu anchored to a button.
export function OverflowMenu({ items, label = "More actions" }: { items: ({ label: string; onClick: () => void; danger?: boolean } | "sep")[]; label?: string }) {
  const [open, setOpen] = React.useState(false);
  // Fixed to the viewport so a scrolling table around the button can't clip it.
  const [at, setAt] = React.useState<{ top: number; right: number } | null>(null);
  const ref = useOutside<HTMLDivElement>(open, () => setOpen(false));
  React.useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => { window.removeEventListener("scroll", close, true); window.removeEventListener("resize", close); };
  }, [open]);
  return (
    <div ref={ref} style={{ position: "relative", display: "inline-block" }}>
      <button className="cx-btn cx-btn-ghost cx-btn-icon" aria-label={label} aria-haspopup="menu" aria-expanded={open}
        onClick={(e) => {
          e.stopPropagation();
          const r = e.currentTarget.getBoundingClientRect();
          const below = window.innerHeight - r.bottom > 240;
          setAt({ top: below ? r.bottom : Math.max(8, r.top - 8 - Math.min(240, items.length * 40)), right: window.innerWidth - r.right });
          setOpen((o) => !o);
        }}><I.overflow /></button>
      {open && at && (
        <div className="cx-menu" role="menu" style={{ position: "fixed", right: at.right, top: at.top, zIndex: 90 }}>
          {items.map((it, i) => it === "sep" ? <hr key={i} /> : (
            <button key={i} role="menuitem" className={it.danger ? "cx-danger" : ""} onClick={(e) => { e.stopPropagation(); setOpen(false); it.onClick(); }}>{it.label}</button>
          ))}
        </div>
      )}
    </div>
  );
}

/// Date, with the time dropped on phones.
export function When({ iso }: { iso: string | null | undefined }) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const M = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  return <span className="cx-num">{d.getDate()} {M[d.getMonth()]} {d.getFullYear()}<span className="cx-hide-sm"> {String(d.getHours()).padStart(2, "0")}:{String(d.getMinutes()).padStart(2, "0")}</span></span>;
}

// ── Search bar + client-side filter (lists on one screen) ─────────────────
export function SearchBar({ value, onChange, placeholder, children }: {
  value: string; onChange: (v: string) => void; placeholder: string; children?: React.ReactNode;
}) {
  return (
    <div className="cx-toolbar">
      <input className="cx-search cx-grow" style={{ paddingLeft: 12 }} placeholder={placeholder} aria-label={placeholder}
        value={value} onChange={(e) => onChange(e.target.value)} />
      {children}
    </div>
  );
}

/// Rows whose `text(row)` contains the query (case-insensitive); all rows when empty.
export function useFilter<T>(rows: T[] | null | undefined, query: string, text: (row: T) => string): T[] {
  const q = query.trim().toLowerCase();
  return React.useMemo(() => (rows ?? []).filter((r) => !q || text(r).toLowerCase().includes(q)),
    // `text` is an inline lambda; the result only depends on rows and the query.
    [rows, q]); // eslint-disable-line react-hooks/exhaustive-deps
}
