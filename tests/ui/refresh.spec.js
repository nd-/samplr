// The list kept up to date: changes on disk reported by `library-changed`,
// a refresh waiting for an edit to end, a slow answer overtaken, and the
// problems of the folder following.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

const emit = (page, name, payload = null) =>
  page.evaluate(([name, payload]) => window.__emit(name, payload), [name, payload]);
const library = (page, action, ...args) =>
  page.evaluate(([action, args]) => window.__library[action](...args), [action, args]);

test("a change on disk keeps the selection and the scroll", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  const list = page.locator("#list");
  await list.evaluate((box) => {
    box.scrollTop = 400;
  });
  await name(page, 20).click();
  await name(page, 22).click({ modifiers: ["Shift"] });
  const scrolled = await list.evaluate((box) => box.scrollTop);

  await library(page, "add", "/lib/Pads/Pad 99.wav");
  await emit(page, "library-changed");
  await expect(page.locator("#status")).toHaveText("51 sample(s)");
  await expect(page.locator("#samples tr.selected")).toHaveCount(3);
  await expect(row(page, 21)).toHaveClass(/selected/);
  expect(await list.evaluate((box) => box.scrollTop)).toBe(scrolled);
});

test("the sample playing stops when it leaves the library", async ({ page }) => {
  await name(page, 0).click();
  await expect(page.locator("#play")).toHaveText("⏸");
  await library(page, "remove", "/lib/Drums/Kick 1.wav");
  await emit(page, "library-changed");
  await expect(page.locator("#play")).toHaveText("▶");
  expect(await calls(page, "stop")).toHaveLength(1);
  await expect(page.locator("#samples tr.selected")).toHaveCount(0);
});

test("a sound only left out by the search keeps playing", async ({ page }) => {
  await name(page, 0).click();
  await page.locator("#search").fill("snare");
  await expect(page.locator("#status")).toHaveText("12 sample(s)");
  await emit(page, "library-changed");
  await expect(page.locator("#samples tr[data-index]")).toHaveCount(12);
  await expect(page.locator("#play")).toHaveText("⏸");
  expect(await calls(page, "stop")).toHaveLength(0);
});

test("a refresh waits while a tempo is being edited", async ({ page }) => {
  const cell = row(page, 30).locator("td.bpm");
  await cell.dblclick();
  await expect(cell.locator("input")).toBeVisible();

  await library(page, "remove", "/lib/Pads/Pad 13.wav");
  await emit(page, "library-changed");
  // Drawing the rows again would close the box: not yet.
  await page.waitForTimeout(100);
  await expect(cell.locator("input")).toBeVisible();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");

  // The edit over, the refresh waiting comes.
  await cell.locator("input").press("Escape");
  await expect(page.locator("#status")).toHaveText("49 sample(s)");
});

test("tags changed elsewhere show in the Tags box", async ({ page }) => {
  await expect(page.locator("#tag-list")).toContainText("No tag yet");
  // `samplr tag` run from a terminal.
  await library(page, "tag", "/lib/Drums/Kick 1.wav", "punchy");
  await emit(page, "library-changed");
  await expect(page.locator("#tag-list li", { hasText: "punchy" }).locator(".count")).toHaveText("1");
  await expect(row(page, 0).locator("td.tags")).toHaveText("punchy");
});

test("a slow answer coming last does not replace a newer search", async ({ page }) => {
  // The search for "kick" answers after the one for "snare", typed later.
  await page.evaluate(() => {
    window.__delay = (command, args) => (command === "search" && args.query === "kick" ? 500 : 0);
  });
  await page.locator("#search").fill("kick");
  await expect.poll(async () => (await calls(page, "search")).at(-1).query).toBe("kick");
  await page.locator("#search").fill("snare");
  await expect(page.locator("#status")).toHaveText("12 sample(s)");
  await expect(name(page, 0)).toHaveText("Snare 1.wav");

  // Well after the slow answer came: still the snares.
  await page.waitForTimeout(700);
  await expect(name(page, 0)).toHaveText("Snare 1.wav");
});

test("problems of the folder following show in the error box", async ({ page }) => {
  await page.evaluate(() => {
    window.__problems.push("/lib/locked: cannot be watched (permission denied)");
  });
  await emit(page, "library-problem");
  await expect(page.locator("#error-text")).toHaveText(
    "/lib/locked: cannot be watched (permission denied)",
  );
  // Asked once: they are not shown again.
  await page.locator("#error-close").click();
  await emit(page, "library-problem");
  await page.waitForTimeout(100);
  await expect(page.locator("#error-box")).toBeHidden();
});
