import { defineConfig, devices } from "@playwright/test";

// Playwright E2E config for the FileHub frontend.
//
// These smoke specs drive a *live* stack — they need the Rust backend on
// :8090 and the Next.js frontend on :3001 (see e2e/README.md).  We do NOT
// spin those up here (no `webServer`) so the same config works against a
// local dev pair or a staging deploy; point elsewhere with E2E_BASE_URL.
//
// The app ships under basePath `/filehub` (next.config.ts), so the baseURL
// carries that prefix and every spec navigates with bare paths
// (`page.goto("/files")` → http://localhost:3001/filehub/files).
export default defineConfig({
  testDir: "./e2e",
  // Login throttling is per-process in the backend (10 fails/min), and the
  // specs share three seed accounts, so keep the suite serial to avoid
  // cross-test interference on the session/login paths.
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: process.env.E2E_BASE_URL ?? "http://localhost:3001/filehub",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],
});
