# API notes

## Folder encryption fields

`GET /api/folders` and `POST /api/folders` return two fields:

- `encrypted`: this folder's own setting only (chosen when it was created).
- `effective_encrypted`: whether new files in this folder are encrypted, after
  inheriting from parent folders and the basket. This is what the Folder
  details screen shows. Use this one to know how a file will be stored.

## Rate limits

Uploads are limited per person per minute (`UPLOAD_RATE_PER_MIN`, default 60).
Limits count in one-minute windows. Past the limit the API answers:

    HTTP 429
    Retry-After: <seconds until the next window>
    {"code":"rate_limited","error":"…","retry_after":<seconds>}

Wait `Retry-After` seconds, then send the same request again.

## Where a file came from

Every upload records its origin once; nobody can edit it afterwards.
`GET /api/files/:id/origin` returns it (the file's Details panel shows it, and
Search matches on these values).

| Field | Form field (`/FileService/upload`) | Header (any upload) |
|---|---|---|
| Source system code | `systemCode` / `sourceSystem` | `X-FileHub-Source` |
| Person who sent it in the source system | `userId` / `username` / `uploadedBy` / `sourceUser` | `X-FileHub-User` |
| Organisation code | `orgCode` / `departmentCode` / `agencyCode` | `X-FileHub-Org` |
| Reference number (request / document no.) | `refNo` / `referenceNo` / `documentNo` / `requestNo` | `X-FileHub-Ref` |
| Source screen | `screenCode` / `sourceScreen` / `pageCode` (else the uploading page) | `X-FileHub-Screen` |

`channel` is `web`, `api` (API key) or `connector`, with the connector's name.

## Statuses per basket

Basket Settings → Statuses has a "Use statuses" switch. Off: status is hidden,
can't be changed (`POST /api/files/:id/status` answers 400), doesn't lock
content, and files don't appear in "Awaiting my review".
