import { expect, type Page } from "@playwright/test";

// Seed accounts bootstrapped on the backend's first run
// (backend/src/auth.rs::SEED_ACCOUNTS).  Mirrors the demo panel on /login.
export const ACCOUNTS = {
  admin:  { email: "admin@acme.go.th",  password: "admin123"  },
  editor: { email: "anong@acme.go.th",  password: "anong123"  },
  viewer: { email: "viewer@acme.go.th", password: "viewer123" },
} as const;

// contract-A12.pdf — seeded in backend/migrations/0002_seed.sql, used by the
// backend integration tests too (tests/common/mod.rs::FILE_001).
export const FILE_001 = "00000000-0000-7000-8000-000000002001";

/// Log in through the real /login form and wait for the post-login redirect
/// off the login page.  The session is an HttpOnly cookie, so the browser
/// carries it on every subsequent navigation within the test.
export async function login(page: Page, who: keyof typeof ACCOUNTS): Promise<void> {
  const { email, password } = ACCOUNTS[who];
  await page.goto("/login");
  // The login page prefills email outside production; set both explicitly so
  // the spec is deterministic regardless of NEXT_PUBLIC_SHOW_DEMO_ACCOUNTS.
  await page.fill('input[type="email"]', email);
  await page.fill('input[type="password"]', password);
  await page.getByRole("button", { name: "Sign in" }).click();
  // On success the page replaces the URL with `next` (default "/"), so just
  // wait until we're no longer on /login.
  await expect(page).not.toHaveURL(/\/login(\?|$)/, { timeout: 15_000 });
}
