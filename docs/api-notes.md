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
