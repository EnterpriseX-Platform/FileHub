import { NextResponse } from "next/server";
import type { NextRequest } from "next/server";

/// Server-side auth gate.
///
/// Previously the app rendered every page anonymously — the dashboard
/// just looked empty because backend reads silently returned [].  That
/// meant a visitor could browse navigation, see system names, see the
/// settings layout, etc. without ever signing in.  This middleware
/// blocks that: if there's no `filehub_session` cookie, redirect to
/// the login page with `?next=` so the user lands back where they
/// started after signing in.
///
/// Things deliberately allowed without a session:
///   - `/filehub/login`            the page that gets the cookie
///   - `/filehub/api/*`            all API calls — proxied to backend, which
///                                 enforces auth itself via `require_session`
///                                 layer.  Intercepting them here would 307
///                                 them into the login HTML page; fetch()
///                                 would follow the redirect and surface a
///                                 200-with-HTML to JS — confusingly "OK".
///                                 Letting them through preserves the real
///                                 401 from the backend so client code can
///                                 react properly (refresh, bounce, etc.).
///   - `/filehub/_next/*`          Next.js build assets
///   - `/filehub/branding/*`       static logo + CSS
///
/// Note we do NOT validate the cookie's signature here — just check it
/// exists. The Rust backend revalidates on every request (sessions
/// table lookup) so a stolen cookie still has to be a live row to do
/// anything. The middleware is a UX gate, not an authentication check.
export function middleware(req: NextRequest) {
  const { pathname, search } = req.nextUrl;

  // Allow public paths through without a session.  Next.js strips the
  // configured basePath from `pathname` before middleware sees it, so
  // the strings below are unprefixed.
  const isPublic =
    pathname === "/login" ||
    pathname.startsWith("/api/") ||      // backend gates its own API
    pathname.startsWith("/_next/") ||
    pathname.startsWith("/branding/") ||
    pathname === "/favicon.ico";

  if (isPublic) return NextResponse.next();

  const hasSession = req.cookies.get("filehub_session")?.value;
  if (hasSession) return NextResponse.next();

  // Bounce to login.  `next=` preserves the deep link so a click on a
  // file URL from an email lands the user back on the file after
  // signing in (handled by login/page.tsx reading the query string).
  //
  // `pathname` already lacks the basePath (Next.js stripped it before
  // middleware ran), so we use it verbatim.  We set `loginUrl.pathname
  // = "/login"`; Next.js auto-prepends the basePath at response time.
  const nextPath = pathname || "/";
  const loginUrl = req.nextUrl.clone();
  loginUrl.pathname = "/login";
  loginUrl.search   = `?next=${encodeURIComponent(nextPath + (search || ""))}`;
  return NextResponse.redirect(loginUrl, 307);
}

export const config = {
  // Match everything except clearly-static asset paths.  Next.js
  // middleware `matcher` strings go through `path-to-regexp`, which
  // does NOT honour the same regex lookahead syntax as plain JS, so
  // we keep the matcher loose and handle the public-path allowlist
  // inside the middleware function above.
  //
  // We register `/` explicitly because the catch-all pattern below
  // doesn't always match the empty path on its own — without this,
  // requests to the dashboard root (`/filehub` post-basePath-strip)
  // would slip through un-authenticated.
  matcher: [
    "/",
    "/((?!_next/static|_next/image).*)",
  ],
};
