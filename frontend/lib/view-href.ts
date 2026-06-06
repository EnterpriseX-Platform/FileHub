/// Translate a `views.filters` JSON column into a `/files?…` URL.  Used by
/// every place that lets the user click a saved view (sidebar SAVED VIEWS
/// section, dashboard Pinned views card).  Only simple equality filters
/// are translated — complex ones ("expires within 30d", "tag in […]")
/// fall back to an unfiltered files page.
export function hrefForView(filtersJson: string | null | undefined): string {
  if (!filtersJson) return "/files";
  let filters: Array<{ field?: string; op?: string; value?: unknown }> = [];
  try { filters = JSON.parse(filtersJson); } catch { return "/files"; }

  const qs: string[] = [];
  for (const f of filters) {
    if (!f.field || f.op !== "is" || typeof f.value !== "string") continue;
    qs.push(`${encodeURIComponent(f.field)}=${encodeURIComponent(f.value)}`);
  }
  return qs.length ? `/files?${qs.join("&")}` : "/files";
}
