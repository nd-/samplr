// The Settings window: opening and closing, the theme, the confirmations,
// the colour palette, and colours given to tags and collections.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

const settings = (page) => page.locator("#settings");
const colors = (page) => page.locator("#color-list li");
const colorName = (page, index) => colors(page).nth(index).locator('input[type="text"]');

async function openSettings(page) {
  await page.locator("#show-settings").click();
  await expect(settings(page)).toBeVisible();
}

const asked = (page) => page.evaluate(() => window.__asked);

test("closed by its ×, by Esc, by a press outside, not by one inside", async ({ page }) => {
  await openSettings(page);
  await settings(page).locator(".close").click();
  await expect(settings(page)).toBeHidden();

  await openSettings(page);
  await page.keyboard.press("Escape");
  await expect(settings(page)).toBeHidden();

  await openSettings(page);
  await settings(page).locator("h3").first().click();
  await expect(settings(page)).toBeVisible();
  await page.mouse.click(5, 5);
  await expect(settings(page)).toBeHidden();
});

test("a theme chosen applies at once and at the next start", async ({ page }) => {
  await openSettings(page);
  await page.locator("#theme").selectOption("dub-dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dub-dark");

  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dub-dark");
  await openSettings(page);
  await expect(page.locator("#theme")).toHaveValue("dub-dark");

  // System: no theme of its own, the computer's light or dark.
  await page.locator("#theme").selectOption("system");
  await expect(page.locator("html")).not.toHaveAttribute("data-theme");
});

test("with confirmations off, a collection is deleted without asking", async ({ page }) => {
  await openSettings(page);
  const ask = page.locator("#ask-before-deleting");
  await expect(ask).toBeChecked();
  await ask.uncheck();
  await page.keyboard.press("Escape");

  await page.locator("#collection-name").fill("Set");
  await page.locator("#collection-name").press("Enter");
  await page.locator("#collection-list li", { hasText: "Set" }).locator("button").click();
  await expect(page.locator("#collection-list li", { hasText: "Set" })).toHaveCount(0);
  expect(await asked(page)).toEqual([]);

  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await openSettings(page);
  await expect(page.locator("#ask-before-deleting")).not.toBeChecked();
});

test("a new colour, named automatically, opens ready to be renamed", async ({ page }) => {
  await openSettings(page);
  await expect(colors(page)).toHaveCount(2);
  await page.locator("#add-color").click();
  await expect(colors(page)).toHaveCount(3);
  const added = colorName(page, 2);
  await expect(added).toBeFocused();
  await expect(added).not.toHaveAttribute("readonly");
  expect((await calls(page, "add_color")).at(-1).name).not.toBe("");

  await added.fill("Dub green");
  await added.press("Enter");
  await expect(added).toHaveAttribute("readonly");
  expect((await calls(page, "update_color")).at(-1).name).toBe("Dub green");
  // Enter did not close the window.
  await expect(settings(page)).toBeVisible();
});

test("a name is read only until ✎, and Esc puts it back", async ({ page }) => {
  await openSettings(page);
  const first = colorName(page, 0);
  await expect(first).toHaveAttribute("readonly");
  await colors(page).nth(0).locator("button", { hasText: "✎" }).click();
  await first.fill("Brick");
  await first.press("Escape");
  await expect(first).toHaveValue("Red");
  expect(await calls(page, "update_color")).toHaveLength(0);
  // Esc in the name did not close the window.
  await expect(settings(page)).toBeVisible();
});

test("an automatic name follows its colour, a name typed by hand stays", async ({ page }) => {
  await openSettings(page);
  await colors(page).nth(1).locator('input[type="color"]').fill("#30a46c");
  await expect.poll(async () => (await calls(page, "update_color")).at(-1)?.name).toBe("Green");

  await colors(page).nth(0).locator("button", { hasText: "✎" }).click();
  await colorName(page, 0).fill("Kicks");
  await colorName(page, 0).press("Enter");
  await colors(page).nth(0).locator('input[type="color"]').fill("#3e63dd");
  await expect
    .poll(async () => (await calls(page, "update_color")).at(-1))
    .toEqual({ id: 1, name: "Kicks", hex: "#3e63dd" });
});

test("deleting a colour says what uses it, and can be declined", async ({ page }) => {
  // A tag coloured from the tag bar's right-click menu.
  await page.locator("#autoplay").uncheck();
  await name(page, 0).click();
  await page.keyboard.press("Control+t");
  await page.keyboard.type("punchy");
  await page.keyboard.press("Enter");
  await page.locator("#tagbar .tag", { hasText: "punchy" }).click({ button: "right" });
  await page.locator("#context-menu button", { hasText: "Red" }).click();
  await expect(row(page, 0).locator("td.tags .chip")).toHaveText("punchy");

  await openSettings(page);
  await page.evaluate(() => {
    window.__answer = false;
  });
  await colors(page).nth(0).locator("button", { hasText: "×" }).click();
  await expect.poll(() => asked(page)).toHaveLength(1);
  expect((await asked(page))[0]).toContain("1 tag(s) and 0 collection(s) use it");
  await expect(colors(page)).toHaveCount(2);

  await page.evaluate(() => {
    window.__answer = true;
  });
  await colors(page).nth(0).locator("button", { hasText: "×" }).click();
  await expect(colors(page)).toHaveCount(1);
  await page.keyboard.press("Escape");
  // The tag lost its colour: plain text again.
  await expect(row(page, 0).locator("td.tags .chip")).toHaveCount(0);
  await expect(row(page, 0).locator("td.tags")).toHaveText("punchy");
});

test("a collection coloured from its right-click menu", async ({ page }) => {
  await page.locator("#collection-name").fill("Set");
  await page.locator("#collection-name").press("Enter");
  const item = page.locator("#collection-list li", { hasText: "Set" });
  await item.click({ button: "right" });
  // The current choice is marked.
  await expect(page.locator("#context-menu button").first()).toHaveText("✓ No colour");
  await page.locator("#context-menu button", { hasText: "Blue" }).click();
  expect((await calls(page, "set_collection_color")).at(-1)).toEqual({ collection: 1, color: 2 });
  await expect(item.locator(".dot")).toHaveCSS("background-color", "rgb(0, 144, 255)");
});

test("the shortcuts are listed with Linux keys here", async ({ page }) => {
  await openSettings(page);
  const shortcuts = page.locator("#shortcuts");
  await expect(shortcuts).toContainText("Ctrl+T");
  await expect(shortcuts).not.toContainText("⌘");
});
