import * as React from "react";

import { Explorer } from "./explorer";

/// Files, the way people think about them:
///   Workspace › Bucket › Folder › File   (+ "Tag folders" — virtual folders built from tags)
/// Client component (tree state, lazy fetches) — `useSearchParams` needs a
/// Suspense boundary for the production build.
export default function ExplorerPage() {
  return (
    <React.Suspense fallback={null}>
      <Explorer />
    </React.Suspense>
  );
}
