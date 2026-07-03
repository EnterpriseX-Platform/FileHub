"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Kbd } from "@/components/primitives";
import { useI18n } from "@/lib/i18n";
import { useViewMode } from "@/lib/view-mode";

/// Unified search/command palette. One entry point for the three things that
/// used to be scattered across a sidebar box, a "Search" page, and an "Ask"
/// page: keyword file search, meaning-based (semantic) search, and a grounded
/// answer. Open with ⌘K / Ctrl+K (or click the trigger). ↑/↓ move the
/// selection, Enter activates it; the "Go to" list jumps to a destination.
type Mode = "files" | "meaning" | "ask";

/// "Go to" destinations differ per shell — from the everyday app the palette
/// must never route into the Admin console (that switch lives in the user
/// menu, explicitly).
const NAV_TARGETS: { labelKey: string; href: string; key: string }[] = [
  { labelKey: "nav.dashboard", href: "/",         key: "dashboard" },
  { labelKey: "nav.files",     href: "/files",    key: "files" },
  { labelKey: "nav.upload",    href: "/upload",   key: "upload" },
  { labelKey: "nav.reports",   href: "/reports",  key: "reports" },
  { labelKey: "nav.activity",  href: "/activity", key: "activity" },
  { labelKey: "nav.shared",    href: "/share",    key: "share" },
  { labelKey: "nav.trash",     href: "/trash",    key: "trash" },
  { labelKey: "nav.settings",  href: "/settings", key: "settings" },
];

const EDAY_NAV_TARGETS: { labelKey: string; href: string; key: string }[] = [
  { labelKey: "eday.nav.home",    href: "/home",    key: "home" },
  { labelKey: "eday.nav.myfiles", href: "/my",      key: "my" },
  { labelKey: "eday.nav.shared",  href: "/shared",  key: "shared" },
  { labelKey: "eday.nav.starred", href: "/starred", key: "starred" },
  { labelKey: "nav.ask",          href: "/ask",     key: "ask" },
  { labelKey: "nav.upload",       href: "/upload",  key: "upload" },
];

export function GlobalSearch() {
  const router = useRouter();
  const { t } = useI18n();
  const { mode: viewMode } = useViewMode();
  const everyday = viewMode === "everyday";
  const navTargets = everyday ? EDAY_NAV_TARGETS : NAV_TARGETS;
  const [openState, setOpen] = React.useState(false);
  const [mode, setMode] = React.useState<Mode>("files");
  const [q, setQ] = React.useState("");
  const [sel, setSel] = React.useState(0);
  const inputRef = React.useRef<HTMLInputElement>(null);

  const open = React.useCallback(() => setOpen(true), []);
  const close = React.useCallback(() => { setOpen(false); setQ(""); setSel(0); }, []);

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

  const term = q.trim();
  const runSearch = () => {
    if (!term) return;
    const dest =
      mode === "ask" ? `/ask?q=${encodeURIComponent(term)}`
      : mode === "meaning" ? `/search?q=${encodeURIComponent(term)}`
      : everyday ? `/my?q=${encodeURIComponent(term)}`
      : `/files?q=${encodeURIComponent(term)}`;
    go(dest);
  };

  const lower = term.toLowerCase();
  const navMatches = lower
    ? navTargets.filter((n) => t(n.labelKey).toLowerCase().includes(lower))
    : navTargets;

  // Flat action list drives keyboard selection: the search-run row (when there
  // is a query) sits at index 0, the nav jumps follow.
  const runVerb = mode === "ask" ? t("cmd.ask") : mode === "meaning" ? t("cmd.findMeaning") : t("cmd.searchFiles");
  const actions: { run: () => void }[] = [
    ...(term ? [{ run: runSearch }] : []),
    ...navMatches.map((n) => ({ run: () => go(n.href) })),
  ];
  const navOffset = term ? 1 : 0;

  // Keep the selection in range as the query filters the list.
  React.useEffect(() => { setSel(0); }, [q, mode]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (actions.length === 0) return;
    if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => (s + 1) % actions.length); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => (s - 1 + actions.length) % actions.length); }
    else if (e.key === "Enter") { e.preventDefault(); actions[Math.min(sel, actions.length - 1)]?.run(); }
    else if (e.key === "Escape") { e.preventDefault(); close(); }
  };

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
          <div className="cmd-panel" onClick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label={t("search.button")}>
            <div className="cmd-input">
              <Ico.search />
              <input
                ref={inputRef}
                value={q}
                onChange={(e) => setQ(e.target.value)}
                placeholder={t("search.placeholder")}
                onKeyDown={onKeyDown}
                role="combobox"
                aria-expanded="true"
                aria-controls="cmd-list"
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

            <div className="cmd-results" id="cmd-list" role="listbox">
              {term && (
                <button
                  type="button"
                  className={"cmd-row cmd-run" + (sel === 0 ? " selected" : "")}
                  role="option"
                  aria-selected={sel === 0}
                  onMouseMove={() => setSel(0)}
                  onClick={runSearch}
                >
                  <Ico.search className="icon sm" />
                  <span>{runVerb}: <strong>{term}</strong></span>
                  <Kbd>↵</Kbd>
                </button>
              )}

              <div className="cmd-label">{t("cmd.goto")}</div>
              {navMatches.length === 0 ? (
                <div className="cmd-empty">{t("cmd.noMatch")}</div>
              ) : (
                navMatches.map((n, i) => {
                  const idx = navOffset + i;
                  return (
                    <button
                      key={n.key}
                      type="button"
                      className={"cmd-row" + (sel === idx ? " selected" : "")}
                      role="option"
                      aria-selected={sel === idx}
                      onMouseMove={() => setSel(idx)}
                      onClick={() => go(n.href)}
                    >
                      <Ico.chevron className="icon sm" />
                      <span>{t(n.labelKey)}</span>
                    </button>
                  );
                })
              )}
            </div>
            <div className="cmd-foot">
              <span><Kbd>↑↓</Kbd> navigate</span>
              <span><Kbd>↵</Kbd> open</span>
              <span><Kbd>esc</Kbd> close</span>
              <span style={{ marginLeft: "auto", display: "inline-flex", alignItems: "center", gap: 4, color: "var(--c-violet)" }}>
                <Ico.sparkle className="icon sm" /> AI-powered
              </span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
