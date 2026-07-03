"use client";

import * as React from "react";
import { createPortal } from "react-dom";

import { Ico } from "@/components/icons";

/// A centered modal dialog, portaled to <body> so it escapes any parent
/// overflow/stacking context (e.g. the settings split-rail). Closes on Escape
/// or a click on the scrim; locks body scroll while open. `wide` widens it for
/// builders (the workflow flow diagram); the body scrolls when it's tall.
export function Modal({
  title,
  onClose,
  children,
  wide,
}: {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  wide?: boolean;
}) {
  const [mounted, setMounted] = React.useState(false);
  React.useEffect(() => {
    setMounted(true);
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", onKey);
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = prev;
    };
  }, [onClose]);

  if (!mounted) return null;
  return createPortal(
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className={"modal-panel" + (wide ? " wide" : "")} role="dialog" aria-modal="true" aria-label={title}>
        <div className="modal-head">
          <div className="t-lg t-semibold">{title}</div>
          <button className="btn xs ghost" onClick={onClose} aria-label="Close"><Ico.x className="icon sm" /></button>
        </div>
        <div className="modal-body">{children}</div>
      </div>
    </div>,
    document.body,
  );
}
