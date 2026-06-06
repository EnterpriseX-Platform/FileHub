// Centralise URL prefixing so changing the basePath only touches this file.
// Server components fetch BACKEND_URL/fh/api/* directly (see lib/api.ts).
// Client components hit /filehub/api/* on the same origin; Next.js rewrites
// that to /fh/api/* on the backend (see next.config.ts).
export const BASE_PATH = "/filehub";

export function apiUrl(path: string): string {
  if (!path.startsWith("/")) path = "/" + path;
  return BASE_PATH + path;
}
