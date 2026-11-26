import type { NextConfig } from "next";

// Force IPv4 by default — `localhost` resolves to `::1` on macOS Sonoma+,
// and the Rust backend binds `0.0.0.0` (v4-only) by default.  Without this
// override, every Next.js rewrite for `/api/*` returns 500 with the cryptic
// `ECONNREFUSED ::1:8090` error.  Override via `BACKEND_URL` env if your
// backend listens on a real hostname.
const BACKEND = process.env.BACKEND_URL || "http://127.0.0.1:8090";

const nextConfig: NextConfig = {
  // Self-contained server bundle for the Docker runtime stage (~10x smaller
  // than shipping node_modules). NOTE: with standalone output this config —
  // including the BACKEND value above — is serialized at BUILD time, so the
  // Docker build must pass the in-cluster BACKEND_URL as a build arg.
  output: "standalone",

  // Everything ships under /filehub so the app composes cleanly with other
  // services on a shared host (e.g., behind nginx).  Next.js prepends the
  // basePath to all <Link> hrefs, static assets, and rewrites automatically.
  basePath: "/filehub",

  // Don't advertise the framework (X-Powered-By: Next.js).
  poweredByHeader: false,

  // Clickjacking: only this site (and portals listed in FRAME_ANCESTORS, e.g.
  // "https://portal.example.go.th") may frame FileHub pages.
  async headers() {
    const ancestors = ["'self'", ...(process.env.FRAME_ANCESTORS ?? "").split(/[\s,]+/).filter(Boolean)].join(" ");
    return [{
      source: "/:path*",
      headers: [
        { key: "Content-Security-Policy", value: `frame-ancestors ${ancestors}; object-src 'none'; base-uri 'self'` },
        { key: "Strict-Transport-Security", value: "max-age=31536000" },
        { key: "X-Content-Type-Options", value: "nosniff" },
        { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
        ...(process.env.FRAME_ANCESTORS ? [] : [{ key: "X-Frame-Options", value: "SAMEORIGIN" }]),
      ],
    }];
  },

  async rewrites() {
    // /filehub/api/* → http://backend/fh/api/* — same-origin from the browser's
    // POV so cookies flow without CORS gymnastics.
    //
    // We register the same destination twice:
    //   1. With basePath baked in (the normal case)
    //   2. With `basePath: false` as a defensive fallback for clients that
    //      have a stale JS chunk caching a fetch like `fetch("/api/views")`
    //      (without the `/filehub` prefix).  Without this fallback those
    //      requests hit Next.js's `_not-found` page and return an HTML 404
    //      that's tedious to debug ("HTTP 404: <!DOCTYPE html>…").
    return [
      { source: "/api/:path*", destination: `${BACKEND}/fh/api/:path*` },
      { source: "/api/:path*", destination: `${BACKEND}/fh/api/:path*`, basePath: false },
    ];
  },

  async redirects() {
    // Catch the bare-path browser navigations our sidebar/topbar `<a>` tags
    // produce (basePath only auto-prefixes <Link>, not <a>) and bounce them
    // onto `/filehub/...`.  We mark `basePath: false` so these patterns match
    // URLs that are NOT already under /filehub, which is exactly when the
    // fix is needed.  `_next/*` and `api/*` are left alone so static assets
    // and the API rewrites keep working.
    return [
      // Old console paths (with or without /filehub) land on today's screens.
      ...([
        ["/files/:id", "/f/:id"], ["/files", "/recent"], ["/upload", "/recent"], ["/share", "/shared"], ["/share/:path*", "/shared"],
        ["/views", "/baskets?tab=virtual"], ["/views/:path*", "/baskets?tab=virtual"], ["/activity", "/admin?tab=log"],
        ["/archive", "/trash"], ["/orgs", "/admin?tab=agencies"], ["/settings", "/admin"], ["/settings/:path*", "/admin"],
        ["/overview", "/recent"], ["/explorer", "/recent"],
      ] as const).flatMap(([from, to]) => [
        { source: from, destination: `/filehub${to}`, basePath: false as const, permanent: false },
        { source: from, destination: to, permanent: false },
      ]),
      { source: "/trash", destination: "/filehub/trash", basePath: false, permanent: false },
      { source: "/login", destination: "/filehub/login", basePath: false, permanent: false },
    ];
  },
};

export default nextConfig;
