"use client";

import * as React from "react";

/// Animates a formatted stat ("19", "517.8 MB", "1,204") counting up from zero
/// on mount. Parses the leading number, animates it with ease-out, and keeps
/// the suffix ("%", " MB") verbatim. Non-numeric values ("—") render as-is.
export function CountUp({ value, ms = 700 }: { value: string; ms?: number }) {
  const parsed = React.useMemo(() => {
    const m = /^([\d,]+(?:\.\d+)?)(.*)$/.exec(value);
    if (!m) return null;
    const num = parseFloat(m[1].replace(/,/g, ""));
    const decimals = m[1].includes(".") ? m[1].split(".")[1].length : 0;
    const commas = m[1].includes(",");
    return { num, decimals, commas, suffix: m[2] };
  }, [value]);

  const [shown, setShown] = React.useState(() => (parsed ? 0 : null));

  React.useEffect(() => {
    if (!parsed) return;
    if (typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setShown(parsed.num);
      return;
    }
    let raf = 0;
    const t0 = performance.now();
    const tick = (t: number) => {
      const p = Math.min(1, (t - t0) / ms);
      const eased = 1 - Math.pow(1 - p, 3);
      setShown(parsed.num * eased);
      if (p < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [parsed, ms]);

  if (!parsed || shown === null) return <>{value}</>;
  let s = shown.toFixed(parsed.decimals);
  if (parsed.commas) s = Number(s).toLocaleString("en-US", { minimumFractionDigits: parsed.decimals });
  return <>{s}{parsed.suffix}</>;
}
