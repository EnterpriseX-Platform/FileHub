import Link from "next/link";
import * as React from "react";

export type Tone = "indigo" | "emerald" | "amber" | "rose" | "violet" | "cyan" | "fuchsia" | "slate";

const toneMap: Record<Tone, string> = {
  indigo: "#4f46e5",
  emerald: "#059669",
  amber: "#d97706",
  rose: "#e11d48",
  violet: "#7c3aed",
  cyan: "#0891b2",
  fuchsia: "#c026d3",
  slate: "#475569",
};

const tones: Tone[] = ["indigo", "emerald", "amber", "rose", "violet", "cyan", "fuchsia", "slate"];

export function Av({
  name = "User",
  sz = "sm",
  tone,
  style,
}: {
  name?: string;
  sz?: "sm" | "lg" | "xl" | "";
  tone?: Tone;
  style?: React.CSSProperties;
}) {
  const hash = [...name].reduce((a, c) => a + c.charCodeAt(0), 0);
  const t: Tone = tone ?? tones[hash % tones.length];
  const initials = name.split(/\s+/).slice(0, 2).map((p) => p[0]).join("").toUpperCase();
  const cls = "av" + (sz === "sm" ? " sm" : sz === "lg" ? " lg" : sz === "xl" ? " xl" : "");
  return (
    <span className={cls} style={{ background: toneMap[t], color: "var(--on-accent)", ...style }}>
      {initials}
    </span>
  );
}

export function WsLogo({ tone = "indigo", children = "F" }: { tone?: Tone; children?: React.ReactNode }) {
  return <span className="ws-logo" style={{ background: toneMap[tone] }}>{children}</span>;
}

const ftLabel: Record<string, string> = {
  pdf: "PDF", doc: "DOC", docx: "DOC", xls: "XLS", xlsx: "XLS",
  csv: "CSV", json: "JS", zip: "ZIP",
  img: "IMG", jpg: "JPG", png: "PNG",
  mp4: "MP4", vid: "MP4",
  fold: "", folder: "",
};

export function Ft({ type = "file", size = "" }: { type?: string; size?: "lg" | "xl" | "" }) {
  const t = (type || "").toLowerCase();
  const label = ftLabel[t] ?? t.toUpperCase().slice(0, 3);
  const cls = "ft" + (size === "lg" ? " lg" : size === "xl" ? " xl" : "") + " " + t;
  if (t === "folder" || t === "fold") {
    return (
      <span className={cls}>
        <svg className="icon" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.6} fill="none" strokeLinecap="round" strokeLinejoin="round">
          <path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2z" />
        </svg>
      </span>
    );
  }
  return <span className={cls}>{label}</span>;
}

export function Pill({
  tone,
  children,
  sm,
  style,
}: {
  tone?: Tone;
  children: React.ReactNode;
  sm?: boolean;
  style?: React.CSSProperties;
}) {
  const cls = "pill" + (tone ? " " + tone : "") + (sm ? " sm" : "");
  return <span className={cls} style={style}>{children}</span>;
}

export function Tag({ children }: { children: React.ReactNode }) {
  return <span className="tag">{children}</span>;
}

export function Prop({
  label,
  icon,
  children,
}: {
  label: string;
  icon?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="prop">
      <div className="k">{icon}{label}</div>
      <div className="v">{children}</div>
    </div>
  );
}

export function SideRow({
  icon,
  label,
  count,
  active,
  indent = 0,
  children,
  href,
}: {
  icon?: React.ReactNode;
  label: React.ReactNode;
  count?: React.ReactNode;
  active?: boolean;
  indent?: 0 | 1 | 2;
  children?: React.ReactNode;
  href?: string;
}) {
  const cls = "side-row" + (active ? " active" : "") + (indent === 1 ? " sub" : indent === 2 ? " subsub" : "");
  const inner = (
    <>
      {icon}
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</span>
      {count != null && <span className="count">{count}</span>}
      {children}
    </>
  );
  // Use next/link so the configured basePath (/filehub) is auto-prepended.
  // The previous plain <a href> bypassed Next.js routing, which sent every
  // sidebar click to a bare path (e.g. /share) that doesn't exist as an app
  // route — see middleware.ts for the defensive fallback.
  if (href) return <Link href={href} className={cls}>{inner}</Link>;
  return <div className={cls}>{inner}</div>;
}

export function SideLabel({ children, action }: { children: React.ReactNode; action?: React.ReactNode }) {
  return <div className="side-label">{children}{action}</div>;
}

export function AvStack({ users, more = 0 }: { users: string[]; more?: number }) {
  return (
    <span className="av-stack">
      {users.map((u, i) => <Av key={i} name={u} />)}
      {more > 0 && (
        <span className="av sm" style={{ background: "var(--bg-strong)", color: "var(--text-muted)", fontWeight: 600 }}>
          +{more}
        </span>
      )}
    </span>
  );
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return <span className="kbd-key">{children}</span>;
}

export function SectionHd({
  title,
  sub,
  action,
}: {
  title: React.ReactNode;
  sub?: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div style={{ display: "flex", alignItems: "baseline", gap: 12, marginBottom: 12 }}>
      <div>
        <div className="t-xl t-semibold">{title}</div>
        {sub && <div className="t-sm t-muted" style={{ marginTop: 2 }}>{sub}</div>}
      </div>
      <div style={{ marginLeft: "auto" }}>{action}</div>
    </div>
  );
}
