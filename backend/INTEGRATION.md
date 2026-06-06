# File Hub — integrating an external app (upload guide)

Full API reference: [`openapi.yaml`](./openapi.yaml) (load it into Swagger UI / Postman / `redocly preview`).

## Where the API lives

Everything is mounted under **`/fh`**. Two equivalent base URLs:

| Caller | Base URL | Example |
|---|---|---|
| Direct to the Rust service | `http://HOST:8090/fh` | `…/fh/api/files` |
| Through the Next.js app (same-origin cookie) | `http://HOST:3001/filehub` | `…/filehub/api/files` |

Browser/SPA clients should use the **`/filehub`** path (same-origin → the cookie is sent automatically). Server-to-server callers can hit either.

## Auth

Two ways to authenticate; both reach the same endpoints.

### API key — recommended for apps / headless integrations

An admin mints a key that authenticates **as a service-account user** — the key inherits that account's role and system access. Send it as a `Bearer` header on every call; there's no session to refresh.

```bash
# Admin (one-time): mint a key bound to a dedicated editor service account.
curl -b admin.cookies -X POST http://HOST:8090/fh/api/api-keys \
  -H 'Content-Type: application/json' \
  -d '{"name":"CI uploader","user_id":"usr_svc","expires_in_days":365}'
# → { "key":"fhk_…", "key_prefix":"fhk_…", "user_id":"usr_svc", "expires_at":… }
#   The plaintext `key` is returned ONCE — store it as a secret now (only its hash is kept).

# Integration: use the key on any private endpoint.
curl -H "Authorization: Bearer fhk_…" http://HOST:8090/fh/api/auth/me   # → the service-account user
```

- Manage keys (admin): `GET /api/api-keys` (metadata only, never the secret), `DELETE /api/api-keys/{id}` (revokes immediately).
- Create the service account first (admin UI or `POST /api/users`) with role **editor** so the key can upload.
- A `viewer`-role key is read-only; an `admin`-role key can do everything — scope the account to least privilege.

### Session cookie — browsers / SPA

Browser clients log in and reuse the **HttpOnly cookie** `filehub_session` (32-byte token, 14-day TTL); it's sent automatically on same-origin `/filehub` calls.

```bash
curl -c fh.cookies -X POST http://HOST:8090/fh/api/auth/login \
  -H 'Content-Type: application/json' -d '{"email":"you@acme.go.th","password":"…"}'
curl -b fh.cookies http://HOST:8090/fh/api/auth/me
```

Mutating calls need role **editor** or **admin** (a `viewer` gets `403`).

## Upload — small / normal files (≤ 64 MiB)

`POST /api/files`, `multipart/form-data`. `system_id` is **required**; repeat `file` to send several. Returns the created file as JSON.

```bash
curl -H "Authorization: Bearer $FH_KEY" -X POST http://HOST:8090/fh/api/files \
  -F system_id=sys_hr \
  -F 'file=@/path/to/report.pdf' \
  -F owner="Reporting Bot" -F status=Draft -F project=Q1-2026 \
  -F 'tags=["report","2026"]'
```

```js
// Browser / Node (with the cookie on the same origin)
const fd = new FormData();
fd.append("system_id", "sys_hr");          // required
fd.append("file", fileBlob, "report.pdf"); // repeatable
fd.append("owner", "Reporting Bot");
fd.append("tags", JSON.stringify(["report"]));
const created = await fetch("/filehub/api/files", {
  method: "POST", credentials: "include", body: fd,
}).then(r => r.json());   // → { id, name, system_id, object_key, ... }
```

Fields: `file` (required, repeatable), `system_id` (required), `org_id`, `folder_id`, `project`, `status` (`Draft|Review|Approved|Archived`), `owner`, `tags` (JSON-array string). The body is **encrypted at rest** (AES-256-GCM) when `STORAGE_ENC_KEY` is set; the `etag` is the MD5 of the plaintext.

## Upload — large / resumable files (TUS 1.0.0)

For big or flaky-network uploads use the TUS endpoint `/api/uploads` (the web client switches at ~50 MiB). Use a TUS client (e.g. `tus-js-client`) and pass the same metadata keys:

```js
import * as tus from "tus-js-client";
const up = new tus.Upload(file, {
  endpoint: `${origin}/filehub/api/uploads`,
  chunkSize: 8 * 1024 * 1024,
  metadata: {                         // base64-encoded on the wire
    filename: file.name,
    content_type: file.type || "application/octet-stream",
    system_id: "sys_hr",              // required
    project: "Q1-2026", status: "Draft", owner: "Reporting Bot",
    tags: JSON.stringify(["report"]),
  },
  onSuccess: () => console.log("done"),
});
up.start();
```

On the final chunk the server runs the same persist pipeline (encrypt → store → DB row → preview/extraction) as the multipart path.

## After upload

The new file shows up in `GET /api/files?system_id=…` (and search, dashboard counts, activity). Other handy calls: `GET /api/files/{id}/download`, `POST /api/files/{id}/share` (tokenized public link), `GET /api/search?q=…` (full-text incl. extracted PDF/txt content). See `openapi.yaml`.
