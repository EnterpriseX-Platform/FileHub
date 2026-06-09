import { expect, test } from "@playwright/test";

import { login } from "./helpers";

// "Search in this view" runs full-text search via ?q= (table-toolbar.tsx
// ViewSearch → buildViewHref → /api/search).  Typing a term and pressing
// Enter navigates to the filtered results.  contract-A12.pdf / -A13 are
// seeded (backend/migrations/0002_seed.sql) so "contract" matches.
test("searching 'contract' in the table filters the results", async ({ page }) => {
  await login(page, "editor");
  await page.goto("/files");

  const box = page.getByPlaceholder("Search in this view…");
  await box.fill("contract");
  await box.press("Enter");

  // The URL carries the query…
  await expect(page).toHaveURL(/[?&]q=contract/);

  // …and the page reflects the search (server component title is
  // `Search: "contract"`) with at least one matching row.
  await expect(page.getByText('Search: "contract"')).toBeVisible();
  await expect(page.getByRole("link", { name: /contract-A12\.pdf/i })).toBeVisible();
});
