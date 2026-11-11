// The layout changed with the mouse and remembered: column widths, the
// collections column folded or widened, the height shared between the
// collections and the tags.
const { test, expect, openFakeLibrary } = require("./helpers");

openFakeLibrary();

// Drags `handle` by (dx, dy) as a person would: press, move, release.
async function drag(page, handle, dx, dy = 0) {
  const box = await handle.boundingBox();
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + dx, y + dy, { steps: 5 });
  await page.mouse.up();
}

const width = async (locator) => (await locator.boundingBox()).width;
const height = async (locator) => (await locator.boundingBox()).height;
const stored = (page, key) => page.evaluate((key) => localStorage.getItem(key), key);

async function restart(page) {
  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
}

test("a column widened by its handle, remembered, back to its default", async ({ page }) => {
  const header = page.locator('th[data-sort="bpm"]');
  const handle = header.locator(".resizer");
  expect(await width(header)).toBeCloseTo(70, 0);
  await drag(page, handle, 50);
  expect(await width(header)).toBeCloseTo(120, 0);
  expect(JSON.parse(await stored(page, "columnWidths")).bpm).toBe(120);
  // Resizing does not sort.
  await expect(header).not.toHaveClass(/sorted/);

  await restart(page);
  expect(await width(header)).toBeCloseTo(120, 0);
  await handle.dblclick();
  expect(await width(header)).toBeCloseTo(70, 0);
});

test("a column is never narrower than 40 pixels", async ({ page }) => {
  const header = page.locator('th[data-sort="type"]');
  await drag(page, header.locator(".resizer"), -300);
  expect(await width(header)).toBeCloseTo(40, 0);
});

test("columns wider than the list make it scroll sideways", async ({ page }) => {
  const list = page.locator("#list");
  const sideways = () => list.evaluate((box) => box.scrollWidth > box.clientWidth);
  // Wide enough for the default widths: at 1400 pixels, with the Info panel
  // and the collections, they already overflow.
  await page.setViewportSize({ width: 2400, height: 800 });
  await expect.poll(sideways).toBe(false);
  await drag(page, page.locator('th[data-sort="name"] .resizer'), 1500);
  expect(await sideways()).toBe(true);
  // The header follows the list sideways.
  await list.evaluate((box) => {
    box.scrollLeft = 300;
    box.dispatchEvent(new Event("scroll"));
  });
  await expect
    .poll(() => page.locator("#list-head").evaluate((head) => head.scrollLeft))
    .toBe(300);
});

test("Ctrl+B folds the collections column, remembered", async ({ page }) => {
  const column = page.locator("#collections");
  const toggle = page.locator("#toggle-collections");
  await page.keyboard.press("ControlOrMeta+b");
  await expect(column).toHaveClass(/collapsed/);
  await expect(toggle).toHaveText("›");
  await expect(page.locator("#collection-list")).toBeHidden();

  await restart(page);
  await expect(column).toHaveClass(/collapsed/);
  await toggle.click();
  await expect(column).not.toHaveClass(/collapsed/);
  await expect(toggle).toHaveText("‹");
});

test("the collections column widened by its edge, within limits", async ({ page }) => {
  const column = page.locator("#collections");
  const handle = page.locator("#collections-resizer");
  const before = await width(column);
  await drag(page, handle, 100);
  expect(await width(column)).toBeCloseTo(before + 100, 0);

  await restart(page);
  expect(await width(column)).toBeCloseTo(before + 100, 0);
  // Never narrower than 120 pixels.
  await drag(page, handle, -500);
  expect(await width(column)).toBeCloseTo(120, 0);
  // A double-click brings the default back, and forgets the width.
  await handle.dblclick();
  expect(await width(column)).toBeCloseTo(before, 0);
  expect(await stored(page, "collectionsWidth")).toBeNull();
});

test("the height shared by collections and tags, moved and remembered", async ({ page }) => {
  const collections = page.locator("#collection-list");
  const tags = page.locator("#tag-list");
  const handle = page.locator("#lists-resizer");
  const shared = (await height(collections)) + (await height(tags));
  const before = await height(collections);
  await drag(page, handle, 0, 60);
  expect(await height(collections)).toBeCloseTo(before + 60, 0);
  // The tags take the rest.
  expect((await height(collections)) + (await height(tags))).toBeCloseTo(shared, 0);

  await restart(page);
  expect(await height(collections)).toBeCloseTo(before + 60, 0);
  await handle.dblclick();
  expect(await height(collections)).toBeCloseTo(before, 0);
  expect(await stored(page, "collectionsHeight")).toBeNull();
});
