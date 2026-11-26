# Running FileHub behind a WAF or CDN

FileHub checks every uploaded file itself. That is what makes it safe to turn
off a WAF's inspection of request bodies on the upload endpoints, and those are
the only endpoints that need it.

## Why uploads need an exception

A WAF that inspects request bodies looks for attack patterns (SQL, script tags,
shell commands). The bytes of real files — PDF, Word, Excel, ZIP, images — are
compressed or binary, so they match those patterns by chance. The WAF then
blocks ordinary uploads, and which files get blocked depends on their content.
How the file is sent makes no difference: raw chunks and standard
`multipart/form-data` are blocked equally often.

## What to configure

Skip **body inspection** (keep every other rule: rate limits, bot rules, IP
rules) for requests that match all of the following:

| | |
|---|---|
| Host | the FileHub host (or `UPLOAD_BASE_URL`, if you use one) |
| Path | `<base>/api/uploads` and `<base>/api/uploads/*` — chunked uploads (TUS 1.0) |
| | `<base>/api/files` and `<base>/api/files/*/versions` — single-request uploads |
| | `<base>/api/upload/*` — upload links shared with people outside |
| | `/FileService/upload`, `/FileService/uploadFile` — only if you use the legacy connector |
| Method | `POST` (FileHub sends PATCH/HEAD/DELETE as POST with `X-HTTP-Method-Override`) |
| Condition | the request carries the `filehub_session` cookie **or** an `Authorization: Bearer` header (the upload links are the only exception) |

`<base>` is the base path, `/filehub` by default.

Also check:

- **Body size limit:** at least 8 MB per request. Chunked uploads send 8 MB at a time; set `UPLOAD_MAX_MB` for the largest single-request upload.
- **Timeouts:** at least 120 s for these paths, because the malware scan runs before the reply.
- **Methods:** a WAF that blocks `PUT`, `PATCH` or `DELETE` is fine; FileHub only needs `GET` and `POST`.

## Scripts and other systems behind such a WAF

A WAF that blocks `PUT`, `PATCH`, `DELETE` or `HEAD` blocks them for scripts too.
Send those calls as `POST` with the real method in `X-HTTP-Method-Override`;
FileHub turns them back into the real method before routing, on every endpoint
(including `/FileService`), with the same access checks and responses:

```
curl -X POST -H "Authorization: Bearer $TOKEN" -H "X-HTTP-Method-Override: DELETE" \
     https://<host>/filehub/api/files/<fileId>
curl -X POST -H "Authorization: Bearer $TOKEN" -H "X-HTTP-Method-Override: PATCH" \
     -H "Content-Type: application/json" -d '{"name":"new.txt"}' \
     https://<host>/filehub/api/files/<fileId>
curl -X POST -H "Authorization: Bearer $TOKEN" -H "X-HTTP-Method-Override: PATCH" \
     -H "Tus-Resumable: 1.0.0" -H "Upload-Offset: 0" \
     -H "Content-Type: application/offset+octet-stream" --data-binary @chunk.bin \
     https://<host>/filehub/api/uploads/<uploadId>
curl -X POST -H "Authorization: Bearer $TOKEN" -H "X-HTTP-Method-Override: HEAD" \
     -H "Tus-Resumable: 1.0.0" -D - https://<host>/filehub/api/uploads/<uploadId>
```

`scripts/test-api.sh` runs the whole suite this way with `METHOD_OVERRIDE=1`.

## What FileHub checks itself

Every upload path runs the same checks: the web app, chunked uploads, new
versions, upload links and the legacy `/FileService`.

- **Who:** a FileHub session, an IAMX/OIDC token, or a connector token. `/FileService` takes requests without a registered token only from callers inside the cluster, or through the ingress when the Bearer token is signed by the legacy issuer's key (`LEGACY_JWT_KEY_URL`).
- **Real type:** the first bytes must match the extension (for example, a `.pdf` must be a PDF and a `.docx` must be a ZIP). Programs (Windows, Linux or macOS executables) are refused unless the basket allows that extension by name.
- **Malware:** set `CLAMAV_HOST=host:3310` and every file is streamed to clamd while it uploads. Infected files are refused. With `CLAMAV_REQUIRED=true`, uploads are also refused while clamd is down; otherwise they go through and a warning is logged.
- **Basket rules:** allowed extensions, maximum size, quota and encryption.

## When the WAF can't make an exception, but already lets uploads through on one path

If the WAF already skips body inspection on one upload path (for example the
legacy `/FileService/upload`), point every browser upload at it:

```
UPLOAD_PATH=/FileService/upload    # needs the legacy connector switched on
```

The web app then sends every request of a chunked upload to that one URL, as a
POST: `X-HTTP-Method-Override` says HEAD/PATCH/DELETE, the session id goes in the
`Upload-Id` header, and each chunk is the file part of an ordinary
`multipart/form-data` body — the file's bytes as they are, nothing re-encoded.
Sign-in, basket rules, folders, versions, encryption and access are the same as
on the usual paths.

## When the WAF can't make an exception: a separate upload host

Point browsers at a host that has no body inspection, for example
`upload.example.go.th` routed straight to the FileHub API:

```
UPLOAD_BASE_URL=https://upload.example.go.th/filehub   # where browsers send file bytes
COOKIE_DOMAIN=.example.go.th                            # share the session with that host
CORS_ORIGIN=https://www.example.go.th                   # the site people open
```

Pages and everything else stay on the main host. Only the file bytes go to the
upload host, and the checks above still run there.

## Checking the setup

Upload a real PDF, Word, Excel and ZIP file of a few MB each, through the
browser and through the real WAF. Each one should appear in the basket. If the
WAF still blocks a file, the response is the WAF's own error page (HTML), not
FileHub's JSON. Take the request ID it shows (for Cloudflare, the Ray ID) to
whoever runs the WAF.
