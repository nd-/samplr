// The list's columns: the header's menu, the column taking the width left,
// sorting by the other columns, and the stripes.
const { test, expect, openFakeLibrary, row, name } = require("./helpers");

openFakeLibrary();

const menu = (page) => page.locator("#context-menu");
const entry = (page, text) => menu(page).locator("button", { hasText: text });
const header = (page, key) => page.locator(`th[data-sort="${key}"]`);

async function openColumnMenu(page) {
  await page.locator("#list-head th").first().click({ button: "right" });
  await expect(menu(page)).toBeVisible();
}

test("the header's menu: Name always shown, several switched in a row", async ({ page }) => {
  await openColumnMenu(page);
  await expect(entry(page, "Name")).toBeDisabled();
  await expect(entry(page, "Name")).toHaveAttribute("title", "Always shown");
  await expect(entry(page, "Show all columns")).toBeDisabled();
  await expect(entry(page, "Codec").locator(".tick")).toHaveText("✓");

  // The menu stays open: two columns hidden one after the other.
  await entry(page, "Codec").click();
  await entry(page, "LUFS").click();
  await expect(entry(page, "Codec").locator(".tick")).toHaveText("");
  await expect(header(page, "codec")).toBeHidden();
  await expect(header(page, "lufs")).toBeHidden();

  await entry(page, "Show all columns").click();
  await expect(header(page, "codec")).toBeVisible();
  await expect(header(page, "lufs")).toBeVisible();
  await expect(entry(page, "Show all columns")).toBeDisabled();
});

test("the last header shown has no line after it", async ({ page }) => {
  await expect(header(page, "folder")).toHaveClass(/last/);
  await openColumnMenu(page);
  await entry(page, "Folder").click();
  await expect(header(page, "tags")).toHaveClass(/last/);
  // Folder shown again: the line comes back after Tags.
  await entry(page, "Folder").click();
  await expect(header(page, "folder")).toHaveClass(/last/);
  await expect(header(page, "tags")).not.toHaveClass(/last/);
});

test("Folder hidden, Name takes the width left, its handle gone", async ({ page }) => {
  const width = async () => (await header(page, "name").boundingBox()).width;
  const before = await width();
  await expect(header(page, "name").locator(".resizer")).toBeVisible();
  await openColumnMenu(page);
  await entry(page, "Folder").click();
  await page.keyboard.press("Escape");
  // Wide enough window: Name grows into the room Folder left.
  await page.setViewportSize({ width: 2400, height: 800 });
  await expect.poll(width).toBeGreaterThan(before + 100);
  await expect(header(page, "name").locator(".resizer")).toBeHidden();
});

test("sorted by length, numbers in their order", async ({ page }) => {
  await header(page, "length").click();
  const lengths = await page
    .locator("#samples tr[data-index] td.number")
    .evaluateAll((cells) => cells.filter((_, i) => i % 4 === 0).map((cell) => cell.textContent));
  const seconds = lengths.map((text) => parseFloat(text));
  expect(seconds).toEqual([...seconds].sort((a, b) => a - b));
});

test("samples without tags stay last, sorted by tags either way", async ({ page }) => {
  await page.evaluate(() => {
    window.__library.tag("/lib/Pads/Pad 13.wav", "airy");
    window.__library.tag("/lib/Drums/Snare 12.wav", "tight");
    window.__emit("library-changed", null);
  });
  await expect(page.locator("#tag-list li", { hasText: "tight" })).toBeVisible();
  await header(page, "tags").click();
  await expect(name(page, 0)).toHaveText("Pad 13.wav");
  await expect(name(page, 1)).toHaveText("Snare 12.wav");
  await header(page, "tags").click();
  await expect(name(page, 0)).toHaveText("Snare 12.wav");
  await expect(name(page, 1)).toHaveText("Pad 13.wav");
});

test("the sort is kept from one search to the next", async ({ page }) => {
  await header(page, "name").click();
  await header(page, "name").click();
  await page.locator("#search").fill("kick");
  await expect(page.locator("#status")).toHaveText("12 sample(s)");
  await expect(name(page, 0)).toHaveText("Kick 12.wav");
  await expect(header(page, "name")).toHaveClass(/sorted-descending/);
});

test("one row in two is striped, after its place in the list", async ({ page }) => {
  await expect(row(page, 0)).not.toHaveClass(/striped/);
  await expect(row(page, 1)).toHaveClass(/striped/);
  // Scrolled, the rows keep their own stripe whatever their place in the
  // table drawn.
  await page.locator("#list").evaluate((list) => {
    list.scrollTop = list.scrollHeight;
  });
  await expect(row(page, 49)).toHaveClass(/striped/);
  await expect(row(page, 48)).not.toHaveClass(/striped/);
});
