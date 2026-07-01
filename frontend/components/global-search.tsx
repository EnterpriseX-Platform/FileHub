"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Kbd } from "@/components/primitives";
import { useI18n } from "@/lib/i18n";

/// Unified search/command palette. One entry point for the three things that
/// used to be scattered across a sidebar box, a "Search" page, and an "Ask"
/// page: keyword file search, meaning-based (semantic) search, and a grounded
/// answer. Open with ⌘K / Ctrl+K (or click the trigger). Enter runs the active
/// mode; the "Go to" list jumps to a destination.
type Mode = "files" | "meaning" | "ask";

const NAV_TARGETS: { label: string; href: string; key: string }[] = [
  { label: "Dashboard", href: "/",         key: "dashboard" },
  { label: "All files", href: "/files",    key: "files" },
  { label: "Upload",    href: "/upload",   key: "upload" },
  { label: "Reports",   href: "/reports",  key: "reports" },
  { label: "Activity",  href: "/activity", key: "activity" },
  { label: "Shared",    href: "/share",    key: "share" },
  { label: "Trash",     href: "/trash",    key: "trash" },
  { label: "Settings",  href: "/settings", key: "settings" },
];

export function GlobalSearch() {
  const router = useRouter();
  const { t } = useI18n();
  const [openState, setOpen] = React.useState(false);
  const [mode, setMode] = React.useState<Mode>("files");
  const [q, setQ] = React.useState("");
  const inputRef = React.useRef<HTMLInputElement>(null);

  const open = React.useCallback(() => setOpen(true), []);
  const close = React.useCallback(() => { setOpen(false); setQ(""); }, []);

  // Global ⌘K / Ctrl+K toggles the palette from anywhere.
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  React.useEffect(() => {
    if (openState) inputRef.current?.focus();
  }, [openState]);

  const go = (href: string) => { close(); router.push(href); };

  const runSearch = () => {
    const term = q.trim();
    if (!term) return;
    const dest =
      mode === "ask" ? `/ask?q=${encodeURIComponent(term)}`
      : mode === "meaning" ? `/search?q=${encodeURIComponent(term)}`
      : `/files?q=${encodeURIComponent(term)}`;
    go(dest);
  };

  const term = q.trim().toLowerCase();
  const matches = term
    ? NAV_TARGETS.filter((n) => n.label.toLowerCase().includes(term))
    : NAV_TARGETS;

  const MODES: { id: Mode; label: string; icon: React.ReactNode }[] = [
    { id: "files",   label: t("nav.files"),  icon: <Ico.search className="icon sm" /> },
    { id: "meaning", label: t("nav.search"), icon: <Ico.sparkle className="icon sm" /> },
    { id: "ask",     label: t("nav.ask"),    icon: <Ico.sparkle className="icon sm" /> },
  ];

  return (
    <div style={{ padding: "8px 12px 4px" }}>
      {/* Trigger — looks like an input, behaves like a button. */}
      <button
        type="button"
        className="field"
        onClick={open}
        style={{ height: 30, width: "100%", fontSize: "var(--t-sm)", cursor: "text", color: "var(--text-subtle)", background: "var(--bg-subtle)", border: "1px solid var(--border)" }}
        aria-label="Open search (Command or Control + K)"
      >
        <Ico.search />
        <span style={{ flex: 1, textAlign: "left" }}>{t("search.button")}…</span>
        <Kbd>⌘K</Kbd>
      </button>

      {openState && (
        <div className="cmd-backdrop" onClick={close} role="presentation">
          <div className="cmd-panel" onClick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Search">
            <div className="cmd-input">
              <Ico.search />
              <input
                ref={inputRef}
                value={q}
                onChange={(e) => setQ(e.target.value)}
                placeholder={t("search.placeholder")}
                onKeyDown={(e) => {
                  if (e.key === "Enter") { e.preventDefault(); runSearch(); }
                  if (e.key === "Escape") { e.preventDefault(); close(); }
                }}
              />
              <Kbd>esc</Kbd>
            </div>

            <div className="cmd-modes">
              {MODES.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  className={"cmd-mode" + (mode === m.id ? " active" : "")}
                  onClick={() => { setMode(m.id); inputRef.current?.focus(); }}
                >
                  {m.icon}{m.label}
                </button>
              ))}
            </div>

            {q.trim() && (
              <button type="button" className="cmd-row cmd-run" onClick={runSearch}>
                <Ico.search className="icon sm" />
                <span>
                  {mode === "ask" ? "Ask" : mode === "meaning" ? "Find by meaning" : "Search files"}: <strong>{q.trim()}</strong>
                </span>
                <Kbd>↵</Kbd>
              </button>
            )}

            <div className="cmd-label">Go to</div>
            {matches.length === 0 ? (
              <div className="cmd-empty">No matching pages.</div>
            ) : (
              matches.map((n) => (
                <button key={n.key} type="button" className="cmd-row" onClick={() => go(n.href)}>
                  <Ico.chevron className="icon sm" />
                  <span>{n.label}</span>
                </button>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}
