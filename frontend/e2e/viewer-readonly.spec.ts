import { expect, test } from "@playwright/test";

import { FILE_001, login } from "./helpers";

// A viewer is read-only: the frontend hides every mutating control behind
// canMutate(role) (lib/roles.ts), and the backend independently returns 403.
// This spec asserts the UI side — no Upload / New folder / Share buttons on
// the files list — and that the (read+comment) comment box still renders on a
// file detail page.
test.describe("viewer read-only gating", () => {
  test("files list hides Upload / New folder for a viewer", async ({ page }) => {
    await login(page, "viewer");
    await page.goto("/files");

    // The list header renders; the page has loaded.
    await expect(page.getByText(/file(s)?/).first()).toBeVisible();

    // canMutate(viewer) === false → these controls are not rendered.
    // exact: true — the sidebar's "My uploads" saved-view link (seed demo
    // data) must not trip this; the gated control is the topbar "Upload".
    await expect(page.getByRole("link", { name: "Upload", exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: /New folder/i })).toHaveCount(0);
  });

  test("file detail hides Share but keeps the comment box for a viewer", async ({ page }) => {
    await login(page, "viewer");
    await page.goto(`/files/${FILE_001}`);

    // Editor+ only — the topbar Share action is gated by canMutate.
    // exact: true so the sidebar's always-visible "Shared" nav link doesn't match.
    await expect(page.getByRole("link", { name: "Share", exact: true })).toHaveCount(0);

    // Everyone who is signed in can comment — the textarea is present
    // (sidecar.tsx CommentsBlock renders it whenever `user` is set).
    await expect(page.getByPlaceholder("Add feedback, a question, or an approval…")).toBeVisible();
  });
});
