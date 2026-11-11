// The list's keys wherever the focus is, and their limits: a text box keeps
// its own keys, a checkbox just used does not take Space, a menu open takes
// Esc.
const { test, expect, calls, openFakeLibrary, row, name } = require("./helpers");

openFakeLibrary();

const selectedRows = (page) => page.locator("#samples tr.selected");

test("Shift+arrows select from the anchor, both ways", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, 5).click();
  await page.keyboard.press("Shift+ArrowDown");
  await page.keyboard.press("Shift+ArrowDown");
  await expect(selectedRows(page)).toHaveCount(3);
  // Back up past the anchor: the range turns round it.
  await page.keyboard.press("Shift+ArrowUp");
  await page.keyboard.press("Shift+ArrowUp");
  await page.keyboard.press("Shift+ArrowUp");
  await expect(selectedRows(page)).toHaveCount(2);
  await expect(row(page, 4)).toHaveClass(/selected/);
  await expect(row(page, 5)).toHaveClass(/selected/);
});

test("the arrows stop at the ends of the list", async ({ page }) => {
  await name(page, 0).click();
  await page.keyboard.press("ArrowUp");
  await expect(row(page, 0)).toHaveClass(/selected/);
  // Nothing played again for a key that went nowhere.
  expect(await calls(page, "play")).toHaveLength(1);
});

test("the arrows browse the list from the search box", async ({ page }) => {
  await page.locator("#search").fill("snare");
  await expect(page.locator("#status")).toHaveText("12 sample(s)");
  await expect(page.locator("#search")).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(row(page, 0)).toHaveClass(/selected/);
  expect((await calls(page, "play")).at(-1).path).toBe("/lib/Drums/Snare 1.wav");
});

test("in the search box, Space and L are typed", async ({ page }) => {
  await name(page, 0).click();
  const played = (await calls(page, "play")).length;
  await page.locator("#search").click();
  await page.keyboard.type("loop l");
  await expect(page.locator("#search")).toHaveValue("loop l");
  await expect(page.locator("#loop")).not.toBeChecked();
  expect(await calls(page, "play")).toHaveLength(played);
  expect(await calls(page, "pause")).toHaveLength(0);
});

test("Space plays even with the focus left on a checkbox", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, 0).click();
  // The checkbox just used keeps the focus: Space must not tick it again.
  await page.locator("#autoplay").focus();
  await page.keyboard.press(" ");
  await expect(page.locator("#autoplay")).not.toBeChecked();
  expect(await calls(page, "play")).toHaveLength(1);
});

test("Ctrl+A in the search box selects its text, not the samples", async ({ page }) => {
  await page.locator("#search").fill("kick");
  await expect(page.locator("#status")).toHaveText("12 sample(s)");
  await page.keyboard.press("Control+a");
  await expect(selectedRows(page)).toHaveCount(0);
  const selection = await page.locator("#search").evaluate(
    (box) => [box.selectionStart, box.selectionEnd],
  );
  expect(selection).toEqual([0, 4]);
});

test("Esc closes a row's menu, and stops nothing", async ({ page }) => {
  await name(page, 0).click();
  await name(page, 0).click({ button: "right" });
  await expect(page.locator("#context-menu")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("#context-menu")).toBeHidden();
  expect(await calls(page, "stop")).toHaveLength(0);
  // The menu closed, Esc stops again.
  await page.keyboard.press("Escape");
  expect(await calls(page, "stop")).toHaveLength(1);
});

test("a menu still waiting for its answer does not open after Esc", async ({ page }) => {
  await page.evaluate(() => {
    window.__delay = (command) => (command === "collections_holding" ? 300 : 0);
  });
  await page.locator("#autoplay").uncheck();
  await name(page, 0).click({ button: "right" });
  await page.keyboard.press("Escape");
  await page.waitForTimeout(500);
  await expect(page.locator("#context-menu")).toBeHidden();
});
