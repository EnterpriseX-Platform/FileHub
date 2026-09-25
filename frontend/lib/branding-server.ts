import { DEFAULT_BRANDING, type Branding } from "./branding-shared";

/// Server-side fetch of the public branding endpoint (root layout / metadata).
export async function loadBranding(): Promise<Branding> {
  const backend = process.env.BACKEND_URL || "http://127.0.0.1:8090";
  try {
    const r = await fetch(`${backend}/fh/api/branding`, { cache: "no-store" });
    if (!r.ok) return DEFAULT_BRANDING;
    const j = (await r.json()) as Partial<Branding>;
    const out = { ...DEFAULT_BRANDING };
    for (const k of Object.keys(out) as (keyof Branding)[]) {
      const v = j[k];
      if (typeof v === "string" && v.trim()) out[k] = v.trim();
    }
    return out;
  } catch {
    return DEFAULT_BRANDING;
  }
}

/// CSS overriding the product accent with the workspace colour (validated
/// `#rrggbb` by the backend).  Derived shades use color-mix so one setting is enough.
export function accentCss(accent: string): string {
  if (!/^#[0-9a-fA-F]{6}$/.test(accent)) return "";
  return `:root{--accent:${accent};--accent-hover:color-mix(in srgb,${accent} 82%,#000);` +
    `--accent-soft:color-mix(in srgb,${accent} 9%,#fff);--accent-border:color-mix(in srgb,${accent} 32%,#fff);` +
    `--accent-text:color-mix(in srgb,${accent} 82%,#000)}`;
}
