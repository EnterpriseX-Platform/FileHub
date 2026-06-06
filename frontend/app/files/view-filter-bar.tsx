import Link from "next/link";

import { Ico } from "@/components/icons";
import { Pill } from "@/components/primitives";

import { FilterMenu, ViewSearch } from "./table-toolbar";
import { buildViewHref, type ViewParams } from "./view-params";

const FILTER_KEYS = ["system_id", "org_id", "status", "project", "owner"] as const;

/// Compact filter bar (status filter + search + removable pills) for the
/// non-table layouts, so Board/Gallery/Calendar/Timeline aren't dead-ends for
/// filtering. `base` keeps navigation on the current layout.
export function ViewFilterBar({ params, base }: { params: ViewParams; base: string }) {
  const active = FILTER_KEYS.filter((k) => params[k]).map((k) => [k, params[k]!] as const);
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
      <FilterMenu params={params} count={active.length} base={base} />
      <ViewSearch params={params} base={base} />
      {active.map(([k, v]) => (
        <Pill key={k} tone="indigo">
          <span style={{ fontWeight: 600 }}>{k}</span> = {`"${v}"`}
          <Link
            href={buildViewHref(base, { ...params, [k]: undefined })}
            title={`Remove ${k} filter`}
            style={{ display: "inline-flex", alignItems: "center", marginLeft: 4, color: "inherit", opacity: 0.65 }}
          >
            <Ico.x className="icon sm" />
          </Link>
        </Pill>
      ))}
    </div>
  );
}
