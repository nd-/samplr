// Tags outside adding them: the tag bar's ×, Backspace and Esc, and the
// Tags box (searching by a click, its order).
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

const chips = (page) => page.locator("#tagbar .tag");
const tagBox = (page) => page.locator("#tag-list li");
const tagItem = (page, tag) => tagBox(page).filter({ hasText: tag });

// Tags given from elsewhere (`samplr tag`), the page told the library
// changed. Kicks 1 to 4: 1 and 2 dry, 1 punchy too, 3 and 4 hard.
test.beforeEach(async ({ page }) => {
  await page.evaluate(() => {
    const tag = (n, tag) => window.__library.tag(`/lib/Drums/Kick ${n}.wav`, tag);
    tag(1, "dry");
    tag(1, "punchy");
    tag(2, "dry");
    tag(3, "hard");
    tag(4, "hard");
    window.__emit("library-changed", null);
  });
  await expect(tagItem(page, "punchy")).toBeVisible();
  await page.locator("#autoplay").uncheck();
});

async function select(page, first, last = first) {
  await name(page, first).click();
  if (last !== first) await name(page, last).click({ modifiers: ["Shift"] });
}

test("the bar shows the tags the whole selection carries", async ({ page }) => {
  await select(page, 0);
  await expect(chips(page)).toHaveText(["dry×", "punchy×"]);
  await select(page, 0, 1);
  await expect(chips(page)).toHaveText(["dry×"]);
});

test("a chip's × takes the tag off the selection", async ({ page }) => {
  await select(page, 0, 1);
  await chips(page).filter({ hasText: "dry" }).locator("button").click();
  await expect(chips(page)).toHaveCount(0);
  expect((await calls(page, "remove_tag")).at(-1)).toEqual({
    paths: ["/lib/Drums/Kick 1.wav", "/lib/Drums/Kick 2.wav"],
    tag: "dry",
  });
  await expect(row(page, 0).locator("td.tags")).toHaveText("punchy");
  // No sample carries it any more: gone from the Tags box.
  await expect(tagItem(page, "dry")).toHaveCount(0);
});

test("Backspace in an empty box takes off the last tag of the bar", async ({ page }) => {
  // Kicks 1 and 2: the last tag they both carry is "dry", not "punchy",
  // which only Kick 1 has.
  await select(page, 0, 1);
  await page.keyboard.press("ControlOrMeta+t");
  await page.keyboard.press("Backspace");
  expect((await calls(page, "remove_tag")).at(-1).tag).toBe("dry");
  await expect(chips(page)).toHaveCount(0);
  await expect(row(page, 0).locator("td.tags")).toHaveText("punchy");

  // Kick 1 alone: its last tag.
  await page.keyboard.press("Escape");
  await select(page, 0);
  await page.keyboard.press("ControlOrMeta+t");
  await page.keyboard.press("Backspace");
  await expect(row(page, 0).locator("td.tags")).toHaveText("");
});

test("Backspace in a box with text only edits the text", async ({ page }) => {
  await select(page, 0);
  await page.keyboard.press("ControlOrMeta+t");
  await page.keyboard.type("ab");
  await page.keyboard.press("Backspace");
  await expect(page.locator("#new-tags")).toHaveValue("a");
  expect(await calls(page, "remove_tag")).toHaveLength(0);
});

test("Esc in the tag box goes back to the search box", async ({ page }) => {
  await select(page, 0);
  await page.keyboard.press("ControlOrMeta+t");
  await page.keyboard.press("Escape");
  await expect(page.locator("#search")).toBeFocused();
});

test("the Tags box counts each tag, and a click searches by it", async ({ page }) => {
  await expect(tagItem(page, "dry").locator(".count")).toHaveText("2");
  await expect(tagItem(page, "hard").locator(".count")).toHaveText("2");
  await expect(tagItem(page, "punchy").locator(".count")).toHaveText("1");

  await tagItem(page, "dry").locator(".name").click();
  await expect(page.locator("#search")).toHaveValue("#dry");
  await expect(page.locator("#status")).toHaveText("2 sample(s)");
  await expect(tagItem(page, "dry")).toHaveClass(/active/);
  // Tags combine.
  await tagItem(page, "punchy").locator(".name").click();
  await expect(page.locator("#search")).toHaveValue("#dry #punchy");
  await expect(page.locator("#status")).toHaveText("1 sample(s)");

  // A second click takes it out.
  await tagItem(page, "dry").locator(".name").click();
  await expect(page.locator("#search")).toHaveValue("#punchy");
  await expect(tagItem(page, "dry")).not.toHaveClass(/active/);
});

test("a tag typed in the search, in any case, is marked in the box", async ({ page }) => {
  await page.locator("#search").fill("kick #HARD");
  await expect(page.locator("#status")).toHaveText("2 sample(s)");
  await expect(tagItem(page, "hard")).toHaveClass(/active/);
  await expect(tagItem(page, "dry")).not.toHaveClass(/active/);
});

test("the Tags box sorted by count, and remembered", async ({ page }) => {
  await expect(tagBox(page).locator(".name")).toHaveText(["dry", "hard", "punchy"]);
  await page.locator("#tag-sort button", { hasText: "Count" }).click();
  // Most used first, then by name.
  await expect(tagBox(page).locator(".name")).toHaveText(["dry", "hard", "punchy"]);
  await page.evaluate(() => {
    window.__library.tag("/lib/Drums/Kick 5.wav", "punchy");
    window.__library.tag("/lib/Drums/Kick 6.wav", "punchy");
    window.__emit("library-changed", null);
  });
  await expect(tagBox(page).locator(".name")).toHaveText(["punchy", "dry", "hard"]);
  await expect(page.locator("#tag-sort button", { hasText: "Count" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );

  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect(page.locator("#tag-sort button", { hasText: "Count" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});
