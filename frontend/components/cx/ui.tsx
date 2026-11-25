"use client";

import * as React from "react";

import type { StatusDef, TagColor } from "@/lib/fh";

// ── Icons (@carbon/icons, 32px grid, filled) ─────────────────────────────
type IP = { size?: number; className?: string; style?: React.CSSProperties };
const svg = (d: React.ReactNode) => function Icon({ size = 16, className, style }: IP) {
  return <svg width={size} height={size} viewBox="0 0 32 32" fill="currentColor" aria-hidden className={className} style={style}>{d}</svg>;
};
const P = {
  folder: <path d="M11.17,6l3.42,3.41.58.59H28V26H4V6h7.17m0-2H4A2,2,0,0,0,2,6V26a2,2,0,0,0,2,2H28a2,2,0,0,0,2-2V10a2,2,0,0,0-2-2H16L12.59,4.59A2,2,0,0,0,11.17,4Z" />,
  folderOpen: <path d="M28,8H16L12.59,4.59A2,2,0,0,0,11.17,4H4A2,2,0,0,0,2,6V26a2,2,0,0,0,2,2H28a2,2,0,0,0,2-2V10A2,2,0,0,0,28,8Zm0,18H4V12H28Z" />,
  doc: <path d="M25.7,9.3l-7-7A.91.91,0,0,0,18,2H8A2,2,0,0,0,6,4V28a2,2,0,0,0,2,2H24a2,2,0,0,0,2-2V10A.91.91,0,0,0,25.7,9.3ZM18,4.4,23.6,10H18ZM24,28H8V4h8v6a2,2,0,0,0,2,2h6Z" />,
  filter: <path d="M18,28H14a2,2,0,0,1-2-2V18.41L4.59,11A2,2,0,0,1,4,9.59V6A2,2,0,0,1,6,4H26a2,2,0,0,1,2,2V9.59A2,2,0,0,1,27.41,11L20,18.41V26A2,2,0,0,1,18,28ZM6,6V9.59l8,8V26h4V17.59l8-8V6Z" />,
  share: <path d="M23,20a5,5,0,0,0-3.89,1.89L11.8,17.32a4.46,4.46,0,0,0,0-2.64l7.31-4.57A5,5,0,1,0,18,7a4.79,4.79,0,0,0,.2,1.32l-7.31,4.57a5,5,0,1,0,0,6.22l7.31,4.57A4.79,4.79,0,0,0,18,25a5,5,0,1,0,5-5ZM23,4a3,3,0,1,1-3,3A3,3,0,0,1,23,4ZM7,19a3,3,0,1,1,3-3A3,3,0,0,1,7,19Zm16,9a3,3,0,1,1,3-3A3,3,0,0,1,23,28Z" />,
};
export const I = {
  chevronDown: svg(<polygon points="16 22 6 12 7.4 10.6 16 19.2 24.6 10.6 26 12 16 22" />),
  chevronRight: svg(<polygon points="22 16 12 26 10.6 24.6 19.2 16 10.6 7.4 12 6 22 16" />),
  chevronLeft: svg(<polygon points="10 16 20 6 21.4 7.4 12.8 16 21.4 24.6 20 26 10 16" />),
  close: svg(<polygon points="24 9.4 22.6 8 16 14.6 9.4 8 8 9.4 14.6 16 8 22.6 9.4 24 16 17.4 22.6 24 24 22.6 17.4 16 24 9.4" />),
  add: svg(<polygon points="17 15 17 8 15 8 15 15 8 15 8 17 15 17 15 24 17 24 17 17 24 17 24 15 17 15" />),
  search: svg(<path d="M29,27.59l-7.55-7.56a11,11,0,1,0-1.41,1.41L27.59,29ZM4,13a9,9,0,1,1,9,9A9,9,0,0,1,4,13Z" />),
  menu: svg(<><rect x="4" y="6" width="24" height="2" /><rect x="4" y="24" width="24" height="2" /><rect x="4" y="12" width="24" height="2" /><rect x="4" y="18" width="24" height="2" /></>),
  bell: svg(<path d="M28.71,19.29,26,16.59V13a10,10,0,0,0-9-9.94V1H15V3.06A10,10,0,0,0,6,13v3.59L3.29,19.29A1,1,0,0,0,3,20v3a1,1,0,0,0,1,1h7v.78a5.21,5.21,0,0,0,4.5,5.22A5,5,0,0,0,21,25V24h7a1,1,0,0,0,1-1V20A1,1,0,0,0,28.71,19.29ZM19,25a3,3,0,0,1-6,0V24h6Zm8-3H5V20.41L7.71,17.71A1,1,0,0,0,8,17V13a8,8,0,0,1,16,0v4a1,1,0,0,0,.29.71L27,20.41Z" />),
  hub: svg(P.folder),
  folder: svg(P.folder),
  folderOpen: svg(P.folderOpen),
  vfolder: svg(P.filter),
  basket: svg(P.folder),
  vbasket: svg(P.filter),
  filter: svg(P.filter),
  upload: svg(<><path d="M6,18,7.41,19.41,15,11.83V30h2V11.83l7.59,7.58L26,18,16,8Z" /><path d="M6,8V4H26V8h2V4a2,2,0,0,0-2-2H6A2,2,0,0,0,4,4V8Z" /></>),
  download: svg(<><path d="M26,24v4H6V24H4v4a2,2,0,0,0,2,2H26a2,2,0,0,0,2-2V24Z" /><path d="M26,14l-1.41-1.41L17,20.17V2H15V20.17L7.41,12.59,6,14,16,24Z" /></>),
  overflow: svg(<><circle cx="16" cy="8" r="2" /><circle cx="16" cy="16" r="2" /><circle cx="16" cy="24" r="2" /></>),
  edit: svg(<><rect x="2" y="26" width="28" height="2" /><path d="M25.4,9c.8-.8.8-2,0-2.8l-3.6-3.6c-.8-.8-2-.8-2.8,0l-15,15V24h6.4Zm-5-5L24,7.6l-3,3L17.4,7ZM6,22V18.4l10-10L19.6,12l-10,10Z" /></>),
  trash: svg(<><rect x="12" y="12" width="2" height="12" /><rect x="18" y="12" width="2" height="12" /><path d="M4,6V8H6V28a2,2,0,0,0,2,2H24a2,2,0,0,0,2-2V8h2V6ZM8,28V8H24V28Z" /><rect x="12" y="2" width="8" height="2" /></>),
  lock: svg(<path d="M24,14H22V8A6,6,0,0,0,10,8v6H8a2,2,0,0,0-2,2V28a2,2,0,0,0,2,2H24a2,2,0,0,0,2-2V16A2,2,0,0,0,24,14ZM12,8a4,4,0,0,1,8,0v6H12ZM24,28H8V16H24Z" />),
  user: svg(<><path d="M16,4a5,5,0,1,1-5,5,5,5,0,0,1,5-5m0-2a7,7,0,1,0,7,7A7,7,0,0,0,16,2Z" /><path d="M26,30H24V25a5,5,0,0,0-5-5H13a5,5,0,0,0-5,5v5H6V25a7,7,0,0,1,7-7h6a7,7,0,0,1,7,7Z" /></>),
  recent: svg(<><path d="M20.59,22,15,16.41V7h2v8.58l5,5.01Z" /><path d="M16,2A13.94,13.94,0,0,0,6,6.23V2H4v8h8V8H7.08A12,12,0,1,1,4,16H2A14,14,0,1,0,16,2Z" /></>),
  shared: svg(P.share),
  share: svg(P.share),
  review: svg(<><polygon points="14 21.414 9 16.413 10.413 15 14 18.586 21.585 11 23 12.415 14 21.414" /><path d="M16,2A14,14,0,1,0,30,16,14,14,0,0,0,16,2Zm0,26A12,12,0,1,1,28,16,12,12,0,0,1,16,28Z" /></>),
  settings: svg(<><path d="M27,16.76c0-.25,0-.5,0-.76s0-.51,0-.77l1.92-1.68A2,2,0,0,0,29.3,11L26.94,7a2,2,0,0,0-1.73-1,2,2,0,0,0-.64.1l-2.43.82a11.35,11.35,0,0,0-1.31-.75l-.51-2.52a2,2,0,0,0-2-1.61H13.64a2,2,0,0,0-2,1.61l-.51,2.52a11.48,11.48,0,0,0-1.32.75L7.43,6.06A2,2,0,0,0,6.79,6,2,2,0,0,0,5.06,7L2.7,11a2,2,0,0,0,.41,2.51L5,15.24c0,.25,0,.5,0,.76s0,.51,0,.77L3.11,18.45A2,2,0,0,0,2.7,21L5.06,25a2,2,0,0,0,1.73,1,2,2,0,0,0,.64-.1l2.43-.82a11.35,11.35,0,0,0,1.31.75l.51,2.52a2,2,0,0,0,2,1.61h4.72a2,2,0,0,0,2-1.61l.51-2.52a11.48,11.48,0,0,0,1.32-.75l2.42.82a2,2,0,0,0,.64.1,2,2,0,0,0,1.73-1L29.3,21a2,2,0,0,0-.41-2.51ZM25.21,24l-3.43-1.16a8.86,8.86,0,0,1-2.71,1.57L18.36,28H13.64l-.71-3.55a9.36,9.36,0,0,1-2.7-1.57L6.79,24,4.43,20l2.72-2.4a8.9,8.9,0,0,1,0-3.13L4.43,12,6.79,8l3.43,1.16a8.86,8.86,0,0,1,2.71-1.57L13.64,4h4.72l.71,3.55a9.36,9.36,0,0,1,2.7,1.57L25.21,8,27.57,12l-2.72,2.4a8.9,8.9,0,0,1,0,3.13L27.57,20Z" /><path d="M16,22a6,6,0,1,1,6-6A5.94,5.94,0,0,1,16,22Zm0-10a3.91,3.91,0,0,0-4,4,3.91,3.91,0,0,0,4,4,3.91,3.91,0,0,0,4-4A3.91,3.91,0,0,0,16,12Z" /></>),
  link: svg(<><path d="M29.25,6.76a6,6,0,0,0-8.5,0l1.42,1.42a4,4,0,1,1,5.67,5.67l-8,8a4,4,0,1,1-5.67-5.66l1.41-1.42-1.41-1.42-1.42,1.42a6,6,0,0,0,0,8.5A6,6,0,0,0,17,25a6,6,0,0,0,4.27-1.76l8-8A6,6,0,0,0,29.25,6.76Z" /><path d="M4.19,24.82a4,4,0,0,1,0-5.67l8-8a4,4,0,0,1,5.67,0A3.94,3.94,0,0,1,19,14a4,4,0,0,1-1.17,2.85L15.71,19l1.42,1.42,2.12-2.12a6,6,0,0,0-8.51-8.51l-8,8a6,6,0,0,0,0,8.51A6,6,0,0,0,7,28a6.07,6.07,0,0,0,4.28-1.76L9.86,24.82A4,4,0,0,1,4.19,24.82Z" /></>),
  arrowUp: svg(<polygon points="16 4 6 14 7.41 15.41 15 7.83 15 28 17 28 17 7.83 24.59 15.41 26 14 16 4" />),
  arrowDown: svg(<polygon points="24.59 16.59 17 24.17 17 4 15 4 15 24.17 7.41 16.59 6 18 16 28 26 18 24.59 16.59" />),
  sort: svg(<><polygon points="16 28 9 21 10.41 19.59 16 25.17 21.59 19.59 23 21 16 28" /><polygon points="16 4 23 11 21.59 12.41 16 6.83 10.41 12.41 9 11 16 4" /></>),
  warning: svg(<><path d="M16,23a1.5,1.5,0,1,0,1.5,1.5A1.5,1.5,0,0,0,16,23Z" /><rect x="15" y="12" width="2" height="9" /><path d="M29,30H3a1,1,0,0,1-.8872-1.4614l13-25a1,1,0,0,1,1.7744,0l13,25A1,1,0,0,1,29,30ZM4.6507,28H27.3493l.002-.0033L16.002,6.1714h-.004L4.6487,27.9967Z" /></>),
  check: svg(<polygon points="13 24 4 15 5.414 13.586 13 21.171 26.586 7.586 28 9 13 24" />),
  move: svg(<polygon points="18 6 16.57 7.393 24.15 15 4 15 4 17 24.15 17 16.57 24.573 18 26 28 16 18 6" />),
  rules: svg(<><rect x="10" y="18" width="8" height="2" /><rect x="10" y="13" width="12" height="2" /><rect x="10" y="23" width="5" height="2" /><path d="M25,5H22V4a2,2,0,0,0-2-2H12a2,2,0,0,0-2,2V5H7A2,2,0,0,0,5,7V28a2,2,0,0,0,2,2H25a2,2,0,0,0,2-2V7A2,2,0,0,0,25,5ZM12,4h8V8H12ZM25,28H7V7h3v3H22V7h3Z" /></>),
  doc: svg(P.doc),
  star: svg(<path d="M16,6.52l2.76,5.58.46,1,1,.15,6.16.89L22,18.44l-.75.73.18,1,1.05,6.13-5.51-2.89L16,23l-.93.49L9.56,26.34l1-6.13.18-1L10,18.44,5.58,14.09l6.16-.89,1-.15.46-1L16,6.52M16,2l-4.55,9.22L1.28,12.69l7.36,7.18L6.9,30,16,25.22,25.1,30,23.36,19.87l7.36-7.17L20.55,11.22Z" />),
  starFilled: svg(<path d="M16,2l-4.55,9.22L1.28,12.69l7.36,7.18L6.9,30,16,25.22,25.1,30,23.36,19.87l7.36-7.17L20.55,11.22Z" />),
  copy: svg(<><path d="M28,10V28H10V10H28m0-2H10a2,2,0,0,0-2,2V28a2,2,0,0,0,2,2H28a2,2,0,0,0,2-2V10a2,2,0,0,0-2-2Z" /><path d="M4,18H2V4A2,2,0,0,1,4,2H18V4H4Z" /></>),
  success: svg(<path d="M16,2A14,14,0,1,0,30,16,14,14,0,0,0,16,2ZM14,21.5908l-5-5L10.5906,15,14,18.4092,21.41,11l1.5957,1.5859Z" />),
  error: svg(<path d="M16,2C8.3,2,2,8.3,2,16s6.3,14,14,14s14-6.3,14-14C30,8.3,23.7,2,16,2z M21.4,23L16,17.6L10.6,23L9,21.4l5.4-5.4L9,10.6L10.6,9l5.4,5.4L21.4,9l1.6,1.6L17.6,16l5.4,5.4L21.4,23z" />),
  grid: svg(<path d="M12,4H6A2,2,0,0,0,4,6v6a2,2,0,0,0,2,2h6a2,2,0,0,0,2-2V6A2,2,0,0,0,12,4Zm0,8H6V6h6Zm14-8H20a2,2,0,0,0-2,2v6a2,2,0,0,0,2,2h6a2,2,0,0,0,2-2V6A2,2,0,0,0,26,4Zm0,8H20V6h6ZM12,18H6a2,2,0,0,0-2,2v6a2,2,0,0,0,2,2h6a2,2,0,0,0,2-2V20A2,2,0,0,0,12,18Zm0,8H6V20h6Zm14-8H20a2,2,0,0,0-2,2v6a2,2,0,0,0,2,2h6a2,2,0,0,0,2-2V20A2,2,0,0,0,26,18Zm0,8H20V20h6Z" />),
  list: svg(<><rect x="4" y="6" width="24" height="2" /><rect x="4" y="12" width="24" height="2" /><rect x="4" y="18" width="24" height="2" /><rect x="4" y="24" width="24" height="2" /></>),
};

/// FileHub mark: purple square with the folder glyph (identity only).
export function Mark({ size = 22 }: { size?: number }) {
  return <span className="cx-mark" style={{ width: size, height: size }}><I.folder size={Math.round(size * 0.64)} /></span>;
}

/// File-type tile: colored page with the extension (PDF / XLS / DOC / ZIP / IMG …).
const FT: Record<string, string> = {
  pdf: "pdf", xls: "xls", xlsx: "xls", csv: "xls", ods: "xls", doc: "doc", docx: "doc", odt: "doc", rtf: "doc", txt: "doc", ppt: "ppt", pptx: "ppt", odp: "ppt",
  zip: "zip", rar: "zip", "7z": "zip", gz: "zip", tar: "zip", png: "img", jpg: "img", jpeg: "img", gif: "img", webp: "img", svg: "img", bmp: "img", tif: "img", tiff: "img",
  mp3: "av", wav: "av", mp4: "av", mov: "av",
};
export function fileKind(name: string): string {
  const m = /\.([^./]+)$/.exec(name);
  return FT[(m?.[1] ?? "").toLowerCase()] ?? "file";
}
export function FileTile({ name }: { name: string }) {
  const m = /\.([^./]+)$/.exec(name);
  const e = (m?.[1] ?? "").toUpperCase().slice(0, 4) || "FILE";
  return <span className={`cx-ftile cx-ft-${fileKind(name)}`} aria-hidden><span>{e}</span></span>;
}

/// Basket color (systems.tone): the palette the New basket dialog offers, plus older tone names.
export const BASKET_COLORS: { value: string; color: string }[] = [
  { value: "purple", color: "var(--cx-brand)" }, { value: "blue", color: "#1192e8" }, { value: "green", color: "#198038" },
  { value: "brown", color: "#8a3800" }, { value: "red", color: "#da1e28" }, { value: "teal", color: "#007d79" }, { value: "magenta", color: "#d02670" },
];
const TONE_ALIAS: Record<string, string> = { violet: "purple", fuchsia: "magenta", rose: "red", cyan: "blue", sky: "blue", emerald: "green", amber: "brown", orange: "brown" };
export function basketColor(tone: string | null | undefined): string {
  if (!tone) return "var(--cx-brand)";
  if (/^#[0-9a-f]{3,8}$/i.test(tone)) return tone;
  const t = TONE_ALIAS[tone] ?? tone;
  return BASKET_COLORS.find((c) => c.value === t)?.color ?? (tone === "slate" ? "#6f6f6f" : "var(--cx-brand)");
}

// ── Button ────────────────────────────────────────────────────────────────
type BtnKind = "primary" | "secondary" | "tertiary" | "ghost" | "danger" | "danger-ghost";
export function Btn({ kind = "secondary", icon, iconOnly, md, children, className = "", ...p }:
  React.ButtonHTMLAttributes<HTMLButtonElement> & { kind?: BtnKind; icon?: React.ReactNode; iconOnly?: boolean; md?: boolean }) {
  return (
    <button type="button" {...p} className={`cx-btn cx-btn-${kind}${iconOnly ? " cx-btn-icon" : ""}${md ? " cx-btn-md" : ""}${kind === "ghost" && icon ? " cx-btn-lead" : ""} ${className}`}>
      {kind === "ghost" && icon && !iconOnly ? <>{icon}{children}</> : <>{children}{icon}</>}
    </button>
  );
}

// ── Tag ───────────────────────────────────────────────────────────────────
export function Tag({ color = "gray", children, onRemove, title, dot = true }: { color?: TagColor | "purple"; children: React.ReactNode; onRemove?: () => void; title?: string; dot?: boolean }) {
  return (
    <span className={`cx-tag cx-tag-${color}`} title={title}>
      {dot && <span className="cx-tag-dot" aria-hidden />}{children}
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
            {t.error ? <I.error size={20} /> : <I.success size={20} />}
            <span>{t.text}</span>
            {t.undo && <button onClick={() => { t.undo?.(); setList((l) => l.filter((x) => x.id !== t.id)); }}>Undo</button>}
            <button aria-label="Dismiss" onClick={() => setList((l) => l.filter((x) => x.id !== t.id))}><I.close size={16} /></button>
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

// ── Tabs ──────────────────────────────────────────────────────────────────
export function Tabs<T extends string>({ tabs, value, onChange, className }: { tabs: { id: T; label: string }[]; value: T; onChange: (t: T) => void; className?: string }) {
  return (
    <div className={`cx-tabs${className ? ` ${className}` : ""}`} role="tablist">
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
export type MenuItem = { label: string; onClick: () => void; danger?: boolean; icon?: React.ReactNode };
export function OverflowMenu({ items, label = "More actions" }: { items: (MenuItem | "sep")[]; label?: string }) {
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
            <button key={i} role="menuitem" className={it.danger ? "cx-danger" : ""} onClick={(e) => { e.stopPropagation(); setOpen(false); it.onClick(); }}>{it.icon}{it.label}</button>
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
      <div className="cx-grow cx-search-wrap"><I.search />
        <input className="cx-search" placeholder={placeholder} aria-label={placeholder}
          value={value} onChange={(e) => onChange(e.target.value)} /></div>
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

// ── Pagination (Carbon): items per page · range · page · prev/next ─────────
export const PAGE_SIZES = [25, 50, 100];
export function Pager({ page, size, total, onPage, onSize, more }: {
  page: number; size: number; total: number | null; onPage: (p: number) => void; onSize: (n: number) => void;
  /// When the total is unknown: whether a next page exists.
  more?: boolean;
}) {
  const from = total === 0 ? 0 : page * size + 1;
  const pages = total == null ? null : Math.max(1, Math.ceil(total / size));
  const to = total == null ? page * size + size : Math.min(total, (page + 1) * size);
  const hasNext = pages == null ? !!more : page + 1 < pages;
  return (
    <div className="cx-pager" role="navigation" aria-label="Pagination">
      <span className="cx-pager-seg cx-pager-size">Items per page:
        <Dropdown value={String(size)} up options={PAGE_SIZES.map((n) => ({ value: String(n), label: String(n) }))} onChange={(v) => onSize(Number(v))} triggerClassName="cx-pager-dd" />
      </span>
      <span className="cx-pager-seg cx-num cx-hide-sm">{total == null ? `${from}–${to}` : `${from}–${to} of ${total} item${total === 1 ? "" : "s"}`}</span>
      <span className="cx-spacer" />
      {pages != null && <span className="cx-pager-seg cx-num">Page {page + 1} of {pages}</span>}
      <button className="cx-pager-nb" aria-label="Previous page" disabled={page === 0} onClick={() => onPage(page - 1)}><I.chevronLeft /></button>
      <button className="cx-pager-nb" aria-label="Next page" disabled={!hasNext} onClick={() => onPage(page + 1)}><I.chevronRight /></button>
    </div>
  );
}

/// Page size remembered per list in this browser (25 / 50 / 100).
export function usePageSize(key: string, initial = 50): [number, (n: number) => void] {
  const [n, setN] = React.useState(initial);
  React.useEffect(() => { try { const v = Number(localStorage.getItem(`fh.page.${key}`)); if (PAGE_SIZES.includes(v)) setN(v); } catch { /* storage off */ } }, [key]);
  return [n, (v: number) => { setN(v); try { localStorage.setItem(`fh.page.${key}`, String(v)); } catch { /* storage off */ } }];
}

/// Empty state: brand mark + one short line (no how-to text).
export function Empty({ children, icon, action }: { children: React.ReactNode; icon?: React.ReactNode; action?: React.ReactNode }) {
  return (
    <div className="cx-empty">
      <div className="cx-empty-mk">{icon ?? <I.folder size={20} />}</div>
      <strong>{children}</strong>
      {action}
    </div>
  );
}

/// On/off switch (Carbon toggle, small).
export function Toggle({ on, onChange, label, disabled }: { on: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return <button type="button" role="switch" aria-checked={on} aria-label={label} disabled={disabled} className="cx-switch" onClick={() => onChange(!on)} />;
}
