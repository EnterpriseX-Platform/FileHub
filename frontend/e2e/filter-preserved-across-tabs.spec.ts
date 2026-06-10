import { expect, test } from "@playwright/test";

import { login } from "./helpers";

// The view params (filters, search, sort, group, columns) are preserved when
// switching layout tabs — buildViewHref threads them into every tab's href
// (view-tabs.tsx).  This spec drives the status filter on the table view,
// switches to Board, and asserts the URL still carries status=Approved.
test("status filter is preserved when switching from Table to Board", async ({ page }) => {
  await login(page, "editor");
  await page.goto("/files");

  // Open the filter popover (button label is "N filters") and pick Approved.
  // The options carry role="menuitem" (popover a11y pass), not "button".
  await page.getByRole("button", { name: /filter/i }).click();
  await page.getByRole("menuitem", { name: "Approved", exact: true }).click();

  // The table view now scopes to status=Approved.
  await expect(page).toHaveURL(/status=Approved/);

  // Switch to the Board layout tab — params must follow.
  // exact: true — "Dashboard", "Q1 2026 Board", even "onboarding-2026.pdf"
  // all contain "board" and trip the default substring match.
  await page.getByRole("link", { name: "Board", exact: true }).click();

  await expect(page).toHaveURL(/\/files\/board/);
  await expect(page).toHaveURL(/status=Approved/);
});
