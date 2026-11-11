// The error box: shown with the status line, closed by its × or by itself
// after 10 s, not while the pointer is on it; its text can be copied.
const { test, expect, openFakeLibrary } = require("./helpers");

openFakeLibrary();

const box = (page) => page.locator("#error-box");

// The page's clock is the test's: 10 s pass at once.
test.beforeEach(async ({ page }) => {
  await page.clock.install();
  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
});

// An error as the folder following reports it.
async function fail(page, message) {
  await page.evaluate((message) => {
    window.__problems.push(message);
    window.__emit("library-problem", null);
  }, message);
  await expect(box(page)).toBeVisible();
}

test("an error shows in its box and in the status line", async ({ page }) => {
  await fail(page, "/lib/locked: cannot be watched");
  await expect(page.locator("#error-text")).toHaveText("/lib/locked: cannot be watched");
  await expect(page.locator("#status")).toHaveText("Error: /lib/locked: cannot be watched");
  await page.locator("#error-close").click();
  await expect(box(page)).toBeHidden();
});

test("the box closes by itself after 10 seconds", async ({ page }) => {
  await fail(page, "first");
  await page.clock.runFor(9_900);
  await expect(box(page)).toBeVisible();
  await page.clock.runFor(200);
  await expect(box(page)).toBeHidden();
});

test("a new error gives the box 10 seconds again", async ({ page }) => {
  await fail(page, "first");
  await page.clock.runFor(8_000);
  await fail(page, "second");
  await page.clock.runFor(8_000);
  await expect(page.locator("#error-text")).toHaveText("second");
  await page.clock.runFor(2_100);
  await expect(box(page)).toBeHidden();
});

test("not while the pointer is on it, 10 seconds again once it leaves", async ({ page }) => {
  await fail(page, "to be copied");
  await box(page).hover();
  await page.clock.runFor(30_000);
  await expect(box(page)).toBeVisible();

  await page.locator("#status").hover();
  await page.clock.runFor(9_900);
  await expect(box(page)).toBeVisible();
  await page.clock.runFor(200);
  await expect(box(page)).toBeHidden();
});

test("the error's text can be selected, unlike the rest of the page", async ({ page }) => {
  await fail(page, "to be copied");
  const select = (selector) =>
    page.locator(selector).evaluate((element) => getComputedStyle(element).userSelect);
  expect(await select("#status")).toBe("none");
  expect(await select("#error-text")).not.toBe("none");
  await page.locator("#error-close").click();
});
