import * as React from "react";

import { Shell } from "@/components/cx/shell";

import "../carbon.css";

/// Basket platform screens (Carbon g10, compact). The older screens keep their
/// own layout; everything under this group shares the new UI shell.
export default function CxLayout({ children }: { children: React.ReactNode }) {
  return (
    <React.Suspense fallback={null}>
      <Shell>{children}</Shell>
    </React.Suspense>
  );
}
