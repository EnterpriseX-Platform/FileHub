"use client";

import { useRouter } from "next/navigation";
import * as React from "react";

import { Ico } from "@/components/icons";
import { Kbd } from "@/components/primitives";

/// Sidebar "Search or jump to…" input — previously a decorative input with
/// no submit handler.  Now navigates to /files?q=<term> which the files
/// page routes through /api/search (FTS over names + tags + extracted text).
export function GlobalSearch() {
  const router = useRouter();
  const [q, setQ] = React.useState("");

  const submit = () => {
    const term = q.trim();
    if (term) router.push(`/files?q=${encodeURIComponent(term)}`);
  };

  return (
    <div style={{ padding: "8px 12px 4px" }}>
      <form
        className="field"
        style={{ height: 30, fontSize: "var(--t-sm)" }}
        onSubmit={(e) => { e.preventDefault(); submit(); }}
      >
        <Ico.search />
        <input
          placeholder="Search files…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          // Keep the ⌘K hint working even though we don't bind it yet.
          onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); submit(); } }}
        />
        <Kbd>⌘K</Kbd>
      </form>
    </div>
  );
}
