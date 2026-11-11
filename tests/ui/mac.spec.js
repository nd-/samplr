// On a Mac: the shortcuts shown with the Mac's keys (⌘ ⇧ ⌫), Cmd doing what
// Ctrl does elsewhere, and the Finder named in the row menu.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

// The page reads the platform once, at its start: set before a reload.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, "platform", { get: () => "MacIntel" });
  });
  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect.poll(() => page.evaluate(() => window.__pending)).toBe(0);
});

test("the tooltips name the Mac's keys", async ({ page }) => {
  await expect(page.locator("#toggle-collections")).toHaveAttribute(
    "title",
    "Hide the collections (⌘B)",
  );
  await expect(page.locator("#toggle-info")).toHaveAttribute("title", "Hide the info (⌘I)");
  await expect(page.locator("th.star")).toHaveAttribute("title", "Favorite (⌘D). Click to sort");
  await expect(row(page, 0).locator("td.star")).toHaveAttribute("title", "Favorite (⌘D)");
});

test("the Settings list the shortcuts with the Mac's keys", async ({ page }) => {
  await page.locator("#show-settings").click();
  const shortcuts = page.locator("#shortcuts");
  await expect(shortcuts.locator("kbd", { hasText: "⌘T" })).toHaveCount(1);
  await expect(shortcuts.locator("kbd", { hasText: "⌘click" })).toHaveCount(1);
  await expect(shortcuts.locator("kbd", { hasText: "⇧click" })).toHaveCount(1);
  await expect(shortcuts.locator("kbd", { hasText: "⌫" })).toHaveCount(1);
  await expect(shortcuts).not.toContainText("Ctrl");
});

test("Cmd does what Ctrl does elsewhere", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  // Cmd+click adds to the selection.
  await name(page, 0).click();
  await name(page, 2).click({ modifiers: ["Meta"] });
  await expect(page.locator("#samples tr.selected")).toHaveCount(2);

  // Cmd+D: favorites.
  await page.keyboard.press("Meta+d");
  await expect(row(page, 2).locator("td.star")).toHaveText("★");
  // Cmd+A: every sample.
  await page.keyboard.press("Meta+a");
  await expect(page.locator("#new-tags")).toHaveAttribute("placeholder", "Add tags to 50 samples");
  // Cmd+T: the tag box.
  await page.keyboard.press("Meta+t");
  await expect(page.locator("#new-tags")).toBeFocused();
  await page.keyboard.press("Escape");
  // Cmd+B and Cmd+I fold the side columns.
  await page.keyboard.press("Meta+b");
  await expect(page.locator("#collections")).toHaveClass(/collapsed/);
  await page.keyboard.press("Meta+i");
  await expect(page.locator("#info")).toHaveClass(/collapsed/);
});

test("Backspace, the Mac's Delete key, takes samples out of a collection", async ({ page }) => {
  await page.locator("#collection-name").fill("Set");
  await page.locator("#collection-name").press("Enter");
  await page.locator("#autoplay").uncheck();
  await name(page, 0).click();
  await name(page, 0).click({ button: "right" });
  await page.locator("#context-menu button", { hasText: "Set" }).click();
  await page.keyboard.press("Escape");
  await page.locator("#collection-list li", { hasText: "Set" }).locator(".name").click();
  await expect(page.locator("#status")).toHaveText("1 sample(s)");

  await name(page, 0).click();
  await page.keyboard.press("Backspace");
  await expect(page.locator("#samples tr[data-index]")).toHaveCount(0);
  expect(await calls(page, "remove_from_collection")).toHaveLength(1);
});

test("the row menu shows the file in the Finder", async ({ page }) => {
  await name(page, 0).click({ button: "right" });
  await expect(page.locator("#context-menu button", { hasText: "Show in Finder" })).toBeVisible();
  await expect(page.locator("#context-menu button", { hasText: "file manager" })).toHaveCount(0);
});
