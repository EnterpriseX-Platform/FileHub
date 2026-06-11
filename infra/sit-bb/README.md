# FileHub → bb SIT Kubernetes (neb-dev)

Deploy artifacts for running FileHub on the NEB SIT cluster, following the
conventions in `SYSTEM_ADMIN/` (two-stage Jenkins → `kubectl set image`,
`avalantglobal/*` images, `component-docker` pull secret, NFS PVs on
`10.1.102.92`, Apache vhost → NodePort).

## What runs where

| Piece | K8s name | Image | Port | Exposure |
|---|---|---|---|---|
| Postgres 16 | `filehub-postgres` | `postgres:16-alpine` | 5432 | ClusterIP |
| Rust API | `neb-filehub-api` | `avalantglobal/filehub-api` | 8090 | ClusterIP |
| Next.js web | `neb-filehub-app` | `avalantglobal/filehub-web` | 3000 | **NodePort 30814** |

Only the web tier is exposed: the app ships under basePath `/filehub` and
Next.js proxies `/filehub/api/*` → `http://neb-filehub-api:8090/fh/api/*`
inside the cluster, so Apache needs exactly one ProxyPass entry and cookies
stay same-origin. **Unlike the Java components there is no APP_PROFILE** —
config is pure env vars from the Secret.

## Order of operations

1. **NFS dirs** — `ssh 10.1.102.92 'mkdir -p /datastore/filehub/postgres /datastore/filehub/storage'`
2. **Fill secrets** — edit `k8s/secret-filehub-api.yaml` (every `REPLACE_ME_*`)
   and the `POSTGRES_PASSWORD` in `k8s/deployment-filehub-postgres.yaml`
   (must match). Generate hex keys with `openssl rand -hex 32`.
3. **Apply** — `kubectl apply -f k8s/` (postgres first boot takes ~10 s;
   the API runs migrations + seeds demo users automatically on first start).
4. **Jenkins jobs** — paste the four `jenkins/*.groovy` into new pipeline
   jobs: `filehub-api`, `filehub-web`, `deploy-filehub-api-to-kube`,
   `deploy-filehub-web-to-kube`. Adjust `GIT_REPO`/path if the repo isn't
   at `bb/core/filehub.git`. Branch is `main` (not `develops`).
5. **Build** — run `filehub-api` then `filehub-web`. Each pushes
   `avalantglobal/filehub-{api,web}:v1.<date>.<time>` and triggers its
   deploy job.
6. **Apache** — splice `apache-vhost-filehub.conf` into `neb-dev.conf`,
   replace `10.x.x.x` with the worker IP the other entries use, then
   `apachectl configtest && systemctl reload apache2`.
7. **Smoke test** — `http://<public-host>/filehub/login`, sign in with the
   seed editor (`anong@acme.go.th` / `anong123`) and **change the seed
   passwords immediately** (they're public in the repo).

## SIT-specific decisions (and what to revisit for UAT/prod)

- **`COOKIE_SECURE=false`** because neb-dev Apache is still plain http.
  Flip to `true` the day the vhost gets TLS, or logins will silently fail.
- **`CORS_ORIGIN`** must be the exact public origin (scheme + host) users
  hit through Apache.
- **Replicas pinned to 1** for the API: the login throttle is per-process
  and blob storage sits on an RWO NFS PV. Scaling out needs a shared
  throttle store + RWX volume (tracked in TODO.md).
- **No LibreOffice in the API image** (~400 MB saved) — office files have
  no PDF preview on SIT, they fall back to download. Add the apt line in
  `backend/Dockerfile` if previews are wanted.
- **No Collabora / MinIO** on SIT — storage backend is `fs` on the NFS PV;
  office editing is off. Both bolt on later without touching the app.
- **`BACKEND_URL` is baked into the web image at build time** (Next.js
  standalone serializes next.config at build). Rebuild the web image if the
  API Service name ever changes; the Jenkins job exposes it as a parameter.
- **NodePort 30814** assumed free (after 30812/30813). Check the vhost
  notes before applying; change in `k8s/deployment-neb-filehub-app.yaml` +
  the Apache block together.
