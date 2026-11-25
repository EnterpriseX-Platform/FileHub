"use client";

import * as React from "react";

import { DEFAULT_BRANDING, withDefaults, type Branding } from "./branding-shared";

export { DEFAULT_BRANDING, withDefaults, type Branding };

/// Client-side branding for pages rendered before sign-in (the endpoint is public).
export function useBranding(): Branding {
  const [b, setB] = React.useState<Branding>(DEFAULT_BRANDING);
  React.useEffect(() => {
    fetch("/filehub/api/branding", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : null))
      .then((j) => setB(withDefaults(j)))
      .catch(() => {});
  }, []);
  return b;
}
