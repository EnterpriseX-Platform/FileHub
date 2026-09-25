// ─── Edge proxies that only allow GET/POST ────────────────────────────────
// Some deployments sit behind an edge proxy (e.g. a CDN/WAF) that answers 403 to
// PATCH/PUT/DELETE before the request ever reaches the app, and reconfiguring
// the WAF is not always an option ⇒ send the request as POST and carry the real
// method in a header; the backend translates it back (method_override).
export function mutate(url: string, method: "DELETE" | "PATCH" | "PUT", init: RequestInit = {}) {
  const headers = new Headers(init.headers as HeadersInit | undefined);
  headers.set("x-http-method-override", method);
  return fetch(url, { ...init, method: "POST", headers });
}
