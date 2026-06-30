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
      { source: "/files",         destination: "/filehub/files",         basePath: false, permanent: false },
      { source: "/search",        destination: "/filehub/search",        basePath: false, permanent: false },
      { source: "/ask",           destination: "/filehub/ask",           basePath: false, permanent: false },
      { source: "/files/:path*",  destination: "/filehub/files/:path*",  basePath: false, permanent: false },
      { source: "/upload",        destination: "/filehub/upload",        basePath: false, permanent: false },
      { source: "/share",         destination: "/filehub/share",         basePath: false, permanent: false },
      { source: "/share/:path*",  destination: "/filehub/share/:path*",  basePath: false, permanent: false },
      { source: "/views",         destination: "/filehub/views",         basePath: false, permanent: false },
      { source: "/views/:path*",  destination: "/filehub/views/:path*",  basePath: false, permanent: false },
      { source: "/activity",      destination: "/filehub/activity",      basePath: false, permanent: false },
      { source: "/reports",       destination: "/filehub/reports",       basePath: false, permanent: false },
      { source: "/archive",       destination: "/filehub/archive",       basePath: false, permanent: false },
      { source: "/trash",         destination: "/filehub/trash",         basePath: false, permanent: false },
      { source: "/orgs",          destination: "/filehub/orgs",          basePath: false, permanent: false },
      { source: "/settings",      destination: "/filehub/settings",      basePath: false, permanent: false },
      { source: "/settings/:path*", destination: "/filehub/settings/:path*", basePath: false, permanent: false },
      { source: "/login",         destination: "/filehub/login",         basePath: false, permanent: false },
    ];
  },
};

export default nextConfig;
