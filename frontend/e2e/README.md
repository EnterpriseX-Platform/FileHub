# Playwright E2E smoke tests

These specs drive a **live** FileHub stack through a real browser. They are
smoke tests for the flows verified during development — viewer read-only
gating, view-param preservation across layout tabs, and in-view search.

## Prerequisites

The suite talks to a running stack — it does **not** start one for you:

1. **Postgres** — `docker compose up -d postgres` (port 5434).
2. **Backend** on `:8090` — `cd backend && cargo run` (runs migrations + seeds
   the demo users/files on first start).
3. **Frontend** on `:3001` — `cd frontend && PORT=3001 npm run dev`
   (the rest of the system, including CORS + the basePath rewrites, expects
   3001 even though `package.json` says `-p 3000`).

The specs sign in with the seed accounts
(`viewer@acme.go.th` / `anong@acme.go.th`) and rely on the seed file
`contract-A12.pdf`, so a freshly seeded DB is required.

## Install browsers (one-time)

`@playwright/test` is a devDependency. After `npm install`, fetch the browser
binaries once:

```bash
cd frontend
npx playwright install chromium
```

## Run

```bash
cd frontend
npm run e2e                       # all specs (headless Chromium)
npm run e2e -- --headed           # watch it drive the browser
npm run e2e -- search.spec.ts     # one spec
```

Point at a different deploy with `E2E_BASE_URL`
(must include the `/filehub` basePath):

```bash
E2E_BASE_URL=https://staging.example.com/filehub npm run e2e
```

## Notes

- baseURL is `http://localhost:3001/filehub` (see `../playwright.config.ts`),
  so specs navigate with bare paths (`page.goto("/files")`).
- The suite runs serially (`workers: 1`) because the backend throttles logins
  per-process and the specs share seed accounts.
