// The sample list: drawing, selection, keyboard, sorting, editing a cell,
// columns, tags and favorites.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

test("only the rows on screen are drawn", async ({ page }) => {
  const drawn = await page.locator("#samples tr[data-index]").count();
  expect(drawn).toBeGreaterThan(10);
  expect(drawn).toBeLessThan(50);
  // Scrolled to the end, the last rows are drawn in their turn.
  await page.locator("#list").evaluate((list) => {
    list.scrollTop = list.scrollHeight;
  });
  await expect(row(page, 49)).toBeVisible();
  await expect(row(page, 0)).toHaveCount(0);
});

test("a click selects and plays, Autoplay off only selects", async ({ page }) => {
  await name(page, 3).click();
  await expect(row(page, 3)).toHaveClass(/selected/);
  await expect.poll(() => calls(page, "play")).toHaveLength(1);
  expect((await calls(page, "play"))[0].path).toBe("/lib/Drums/Kick 4.wav");

  await page.locator("#autoplay").uncheck();
  await name(page, 5).click();
  await expect(row(page, 5)).toHaveClass(/selected/);
  await expect(row(page, 3)).not.toHaveClass(/selected/);
  expect(await calls(page, "play")).toHaveLength(1);
});

test("the arrow keys move and play", async ({ page }) => {
  await name(page, 0).click();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(row(page, 2)).toHaveClass(/selected/);
  await expect.poll(async () => (await calls(page, "play")).map((c) => c.path)).toEqual([
    "/lib/Drums/Kick 1.wav",
    "/lib/Drums/Kick 2.wav",
    "/lib/Drums/Kick 3.wav",
  ]);
});

test("multiple selection with Shift, Ctrl and Ctrl+A", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, 2).click();
  await name(page, 5).click({ modifiers: ["Shift"] });
  await expect(page.locator("#samples tr.selected")).toHaveCount(4);
  // Ctrl+click takes one out.
  await name(page, 3).click({ modifiers: ["Control"] });
  await expect(page.locator("#samples tr.selected")).toHaveCount(3);
  await expect(row(page, 3)).not.toHaveClass(/selected/);

  // Ctrl+A: every sample, those off screen included.
  await page.keyboard.press("Control+a");
  await expect(page.locator("#new-tags")).toHaveAttribute("placeholder", "Add tags to 50 samples");
});

test("sorting by name is natural, a second click reverses it", async ({ page }) => {
  const header = page.locator('th[data-sort="name"]');
  await header.click();
  // "Bass 2" before "Bass 10", whatever the database's order.
  await expect(name(page, 0)).toHaveText("Bass 1 120BPM Am.wav");
  await expect(name(page, 1)).toHaveText("Bass 2 120BPM Am.wav");
  await expect(header).toHaveClass(/sorted-ascending/);
  await header.click();
  await expect(name(page, 0)).toHaveText("Snare 12.wav");
  await expect(header).toHaveClass(/sorted-descending/);
});

test("samples without a tempo stay last whichever the direction", async ({ page }) => {
  const header = page.locator('th[data-sort="bpm"]');
  await header.click();
  await expect(name(page, 0)).toContainText("Bass");
  await header.click();
  await expect(name(page, 0)).toContainText("Bass");
});

test("typing in the search box searches", async ({ page }) => {
  await page.locator("#search").fill("snare");
  await expect(page.locator("#status")).toHaveText("12 sample(s)");
  expect((await calls(page, "search")).at(-1).query).toBe("snare");
});

test("a tempo typed in its cell, a refused one keeps the box open", async ({ page }) => {
  const cell = row(page, 0).locator("td.bpm");
  await cell.dblclick();
  const box = cell.locator("input");
  await box.fill("999");
  await box.press("Enter");
  await expect(box).toHaveClass(/invalid/);
  await expect(page.locator("#error-box")).toBeVisible();

  // Still in the box, corrected. (Closing the error first would leave the
  // box, which gives the old value back.)
  await box.fill("128");
  await box.press("Enter");
  await expect(row(page, 0).locator("td.bpm")).toHaveText("128");
  await page.locator("#error-close").click();
  expect((await calls(page, "set_tempo")).at(-1)).toEqual({
    paths: ["/lib/Drums/Kick 1.wav"],
    value: "128",
  });
});

test("Esc leaves a tempo cell unchanged", async ({ page }) => {
  const cell = row(page, 24).locator("td.bpm");
  const before = await cell.textContent();
  await cell.dblclick();
  await cell.locator("input").fill("90");
  await cell.locator("input").press("Escape");
  await expect(row(page, 24).locator("td.bpm")).toHaveText(before);
  expect(await calls(page, "set_tempo")).toHaveLength(0);
});

test("tags added from the tag bar, to the whole selection", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, 0).click();
  await name(page, 1).click({ modifiers: ["Shift"] });
  await page.keyboard.press("Control+t");
  await expect(page.locator("#new-tags")).toBeFocused();
  await page.keyboard.type("Punchy dry");
  await page.keyboard.press("Enter");
  await expect(page.locator("#tagbar .tag")).toHaveText(["dry×", "punchy×"]);
  await expect(row(page, 1).locator("td").filter({ hasText: "punchy" })).toHaveCount(1);
  expect((await calls(page, "add_tags")).at(-1).paths).toHaveLength(2);
});

test("the star toggles a favorite without selecting nor playing", async ({ page }) => {
  const star = row(page, 4).locator("td.star");
  await star.click();
  await expect(star).toHaveText("★");
  await expect(row(page, 4)).not.toHaveClass(/selected/);
  expect(await calls(page, "play")).toHaveLength(0);
  expect((await calls(page, "set_favorite")).at(-1)).toEqual({
    paths: ["/lib/Drums/Kick 5.wav"],
    favorite: true,
  });
});

test("a column hidden from the header's menu stays hidden after a restart", async ({ page }) => {
  await page.locator("#list-head th").first().click({ button: "right" });
  await page.locator("#context-menu button", { hasText: "Codec" }).click();
  await expect(page.locator('th[data-sort="codec"]')).toBeHidden();
  await expect(row(page, 0).locator("td.codec")).toHaveCount(0);

  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect(page.locator('th[data-sort="codec"]')).toBeHidden();
});
