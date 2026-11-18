# Browser E2E

`flows.spec.ts` walks the everyday screens (sign in, a large basket, nested
folders and the folder tree, upload, file details, search, delete and restore,
every Admin section, sign out), times each step and fails on a console error or
a step slower than `STEP_BUDGET_MS` (default 2000 ms).

Run it against a production build — dev mode compiles pages on first visit:

    BACKEND_URL=http://127.0.0.1:8190 npx next build
    BACKEND_URL=http://127.0.0.1:8190 npx next start -p 3102
    E2E_BASE_URL=http://localhost:3102/filehub/ npx playwright test

Needs the backend with the demo seed accounts (DEMO_MODE=1).
