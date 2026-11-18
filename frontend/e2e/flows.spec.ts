import { expect, test, type Page } from "@playwright/test";

// End-to-end through the screens people use, timing each step.
// Needs the backend (seed accounts) and the frontend; run against a production
// build so the numbers mean something:
//   E2E_BASE_URL=http://localhost:3102/filehub npx playwright test e2e/flows.spec.ts
// A step slower than STEP_BUDGET_MS fails the run; console errors fail it too.

const STEP_BUDGET_MS = Number(process.env.STEP_BUDGET_MS ?? 2000);
const tag = `e2e-${Date.now().toString(36)}`;
const times: { step: string; ms: number }[] = [];

async function timed<T>(step: string, fn: () => Promise<T>): Promise<T> {
  const t = Date.now();
  const out = await fn();
  times.push({ step, ms: Date.now() - t });
  return out;
}

test.describe.configure({ mode: "serial" });

test("everyday flows are quick and error-free", async ({ page }) => {
  test.setTimeout(180_000);
  const errors: string[] = [];
  page.on("console", (m) => { if (m.type() === "error" && !/favicon|401 \(Unauthorized\)/.test(m.text())) errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(String(e)));

  // ── sign in ──
  await page.goto("login");
  await page.getByLabel("Email").fill("admin@acme.go.th");
  await page.getByLabel("Password").fill("admin123");
  await timed("sign in → Recent", async () => {
    await page.getByRole("button", { name: "Sign in" }).click();
    await expect(page.getByRole("heading", { name: "Recent" })).toBeVisible();
  });

  // ── a big basket (tens of thousands of files) ──
  await timed("open a basket with ~78k files", async () => {
    await page.getByRole("link", { name: "All baskets" }).first().click();
    await page.getByRole("link", { name: "Document Mgmt" }).first().click();
    await expect(page.locator("table.cx-table tbody tr").nth(5)).toBeVisible();
  });

  // ── a fresh basket for this run ──
  const sys = await page.evaluate(async (t) => {
    const r = await fetch("/filehub/api/systems", { method: "POST", credentials: "include", headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: `E2E ${t}`, bucket: t, tone: "slate" }) });
    return (await r.json()).id as string;
  }, tag);
  await timed("open the new basket", async () => {
    await page.goto(`b/${sys}`);
    await expect(page.getByRole("heading", { name: `E2E ${tag}` })).toBeVisible();
  });

  // ── nested folders + the folder tree ──
  const newFolder = async (name: string) => {
    await page.getByRole("button", { name: "New folder" }).click();
    await page.getByRole("dialog").locator("input").fill(name);
    await page.getByRole("button", { name: "Create folder" }).click();
    await expect(page.locator("table.cx-table").getByRole("link", { name })).toBeVisible();
  };
  await timed("create folder", () => newFolder("Reports"));
  await timed("open folder", async () => {
    await page.locator("table.cx-table").getByRole("link", { name: "Reports" }).click();
    await expect(page.getByRole("heading", { name: "Reports" })).toBeVisible();
  });
  await timed("create folder inside a folder", () => newFolder("2569"));
  await page.locator("table.cx-table").getByRole("link", { name: "2569" }).click();
  await expect(page.getByRole("heading", { name: "2569" })).toBeVisible();
  const tree = page.getByRole("navigation", { name: "Folders" });
  await expect(tree.getByRole("link", { name: "Reports" })).toBeVisible();
  await expect(tree.getByRole("link", { name: "2569" })).toBeVisible();

  // ── upload ──
  const fileName = `${tag}-budget.pdf`;
  await timed("upload a file", async () => {
    await page.getByRole("button", { name: "Upload" }).click();
    await page.getByRole("dialog").locator('input[type="file"]').setInputFiles({ name: fileName, mimeType: "application/pdf", buffer: Buffer.from("%PDF-1.4 e2e " + "x".repeat(50_000)) });
    await page.getByRole("button", { name: /^Upload 1 file$/ }).click();
    await expect(page.locator("table.cx-table").getByText(fileName)).toBeVisible();
  });
  const dialog = page.getByRole("dialog");
  if (await dialog.isVisible()) await dialog.getByRole("button", { name: /Close|Cancel/ }).click();

  // ── file details ──
  await timed("open file details", async () => {
    await page.locator("table.cx-table").getByRole("button", { name: fileName }).click();
    await expect(page.getByRole("complementary", { name: "File details" })).toBeVisible();
  });
  await page.getByRole("button", { name: "Close details" }).click();
  await expect(page.getByRole("complementary", { name: "File details" })).toBeHidden();

  // ── jump through the tree ──
  await timed("folder tree → basket root", async () => {
    await tree.getByRole("link", { name: `E2E ${tag}` }).click();
    await expect(page.getByRole("heading", { name: `E2E ${tag}` })).toBeVisible();
  });

  // ── search ──
  await timed("search all files", async () => {
    await page.getByPlaceholder("Search all files").fill(fileName);
    await page.getByPlaceholder("Search all files").press("Enter");
    await expect(page.getByText(fileName).first()).toBeVisible();
  });

  // ── delete → Trash → restore ──
  await page.goto(`b/${sys}`);
  await page.locator("table.cx-table").getByRole("link", { name: "Reports" }).click();
  await page.locator("table.cx-table").getByRole("link", { name: "2569" }).click();
  const row = page.locator("table.cx-table tbody tr", { hasText: fileName });
  await timed("delete a file", async () => {
    await row.locator('input[type="checkbox"]').check();
    await page.getByRole("toolbar", { name: "Selection actions" }).getByRole("button", { name: "Delete" }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
    await expect(row).toHaveCount(0);
  });
  await timed("restore from Trash", async () => {
    await page.getByRole("link", { name: "Trash" }).click();
    const t = page.locator("table.cx-table tbody tr", { hasText: fileName });
    await t.getByRole("button", { name: "Restore" }).click();
    await expect(t).toHaveCount(0);
  });

  // ── admin sections ──
  await page.getByRole("link", { name: "Admin" }).click();
  for (const s of ["People", "Agencies", "Keys", "Connectors", "Activity log", "Settings", "Baskets"]) {
    await timed(`admin: ${s}`, async () => {
      await page.getByRole("navigation", { name: "Admin sections" }).getByRole("button", { name: s }).click();
      await expect(page.locator(".cx-admin-body")).not.toBeEmpty();
      await page.waitForLoadState("networkidle");
    });
  }

  // ── sign out ──
  await page.getByRole("button", { name: /admin/i }).last().click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();

  // ── report ──
  console.log("\n step".padEnd(42) + "ms");
  for (const t of times) console.log(` ${t.ms > STEP_BUDGET_MS ? "SLOW" : "ok  "} ${t.step.padEnd(36)} ${t.ms}`);
  expect(errors, `console errors:\n${errors.join("\n")}`).toEqual([]);
  expect(times.filter((t) => t.ms > STEP_BUDGET_MS), "steps over budget").toEqual([]);
});
