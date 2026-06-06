export function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes < 1024 ** 4) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  return `${(bytes / 1024 ** 4).toFixed(1)} TB`;
}

export function fmtCount(n: number): string {
  return n.toLocaleString("en-US");
}

export function fmtAgo(iso: string): string {
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return iso;
  const diff = (Date.now() - t) / 1000;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 604800) return `${Math.floor(diff / 86400)}d ago`;
  if (diff < 2592000) return `${Math.floor(diff / 604800)}w ago`;
  if (diff < 31536000) return `${Math.floor(diff / 2592000)}mo ago`;
  return `${Math.floor(diff / 31536000)}y ago`;
}

export function parseJsonArray(s: string | null | undefined): string[] {
  if (!s) return [];
  try {
    const x = JSON.parse(s);
    return Array.isArray(x) ? x.map(String) : [];
  } catch { return []; }
}

export function statusTone(status: string): "amber" | "emerald" | "slate" | "rose" | "indigo" | "violet" | "cyan" {
  switch (status.toLowerCase()) {
    case "review": return "amber";
    case "approved": return "emerald";
    case "draft": return "slate";
    case "active": return "emerald";
    case "archived": return "slate";
    case "inactive": return "slate";
    case "rejected": return "rose";
    default: return "indigo";
  }
}
