"use client";

import * as React from "react";

import { Ico } from "@/components/icons";

type Tone = "success" | "error" | "info";
type Toast = { id: number; message: string; tone: Tone };
type Ctx = { toast: (message: string, tone?: Tone) => void };

const ToastCtx = React.createContext<Ctx>({ toast: () => {} });

/// Fire-and-forget feedback: `const { toast } = useToast(); toast("Uploaded")`.
/// Renders a bottom-right stack; each toast slides in and auto-dismisses.
export const useToast = () => React.useContext(ToastCtx);

const TTL_MS = 3600;

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [items, setItems] = React.useState<Toast[]>([]);
  const nextId = React.useRef(1);

  const toast = React.useCallback((message: string, tone: Tone = "success") => {
    const id = nextId.current++;
    setItems((s) => [...s.slice(-3), { id, message, tone }]);
    setTimeout(() => setItems((s) => s.filter((t) => t.id !== id)), TTL_MS);
  }, []);

  return (
    <ToastCtx.Provider value={{ toast }}>
      {children}
      {items.length > 0 && (
        <div className="toast-stack" role="status" aria-live="polite">
          {items.map((t) => (
            <div key={t.id} className={`toast ${t.tone}`}>
              <span className="toast-ic">
                {t.tone === "success" ? <Ico.check className="icon sm" />
                  : t.tone === "error" ? <Ico.warning className="icon sm" />
                  : <Ico.bell className="icon sm" />}
              </span>
              <span className="t-sm">{t.message}</span>
            </div>
          ))}
        </div>
      )}
    </ToastCtx.Provider>
  );
}
