// Favorites: Ctrl+D on a selection, and the ★ Favorites filter.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

const star = (page, index) => row(page, index).locator("td.star");
const filter = (page) => page.locator("#favorites-only");

async function select(page, first, last = first) {
  await page.locator("#autoplay").uncheck();
  await name(page, first).click();
  if (last !== first) await name(page, last).click({ modifiers: ["Shift"] });
}

test("Ctrl+D makes the whole selection favorites, unless all already are", async ({ page }) => {
  // One of the three is already a favorite: all become favorites.
  await star(page, 1).click();
  await select(page, 0, 2);
  await page.keyboard.press("ControlOrMeta+d");
  for (const index of [0, 1, 2]) await expect(star(page, index)).toHaveText("★");
  expect((await calls(page, "set_favorite")).at(-1)).toEqual({
    paths: ["/lib/Drums/Kick 1.wav", "/lib/Drums/Kick 2.wav", "/lib/Drums/Kick 3.wav"],
    favorite: true,
  });

  // All of them are: none is any more.
  await page.keyboard.press("ControlOrMeta+d");
  for (const index of [0, 1, 2]) await expect(star(page, index)).toHaveText("☆");
  expect((await calls(page, "set_favorite")).at(-1).favorite).toBe(false);
});

test("Ctrl+D with nothing selected does nothing", async ({ page }) => {
  await page.locator("#list").focus();
  await page.keyboard.press("ControlOrMeta+d");
  expect(await calls(page, "set_favorite")).toHaveLength(0);
});

test("the ★ Favorites button shows the favorites only", async ({ page }) => {
  await star(page, 3).click();
  await star(page, 30).click();
  await filter(page).click();
  await expect(filter(page)).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#status")).toHaveText("2 sample(s)");
  expect((await calls(page, "search")).at(-1).favorites).toBe(true);
  // With a search too.
  await page.locator("#search").fill("kick");
  await expect(page.locator("#status")).toHaveText("1 sample(s)");

  await page.locator("#search").fill("");
  await filter(page).click();
  await expect(filter(page)).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
});

test("a sample no longer a favorite stays in the filtered list until the next search", async ({
  page,
}) => {
  await star(page, 3).click();
  await star(page, 4).click();
  await filter(page).click();
  await expect(page.locator("#status")).toHaveText("2 sample(s)");

  // Its star off, the row stays: the list does not jump under the pointer.
  await star(page, 0).click();
  await expect(star(page, 0)).toHaveText("☆");
  await expect(page.locator("#samples tr[data-index]")).toHaveCount(2);

  await page.locator("#search").fill("kick");
  await expect(page.locator("#status")).toHaveText("1 sample(s)");
});
