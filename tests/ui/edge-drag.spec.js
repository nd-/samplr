// Drag and drop at its edges: the 5-pixel threshold, where a drag may start,
// what it offers, and the drop events Tauri reports in every order and at
// every scale.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

const drags = (page) => page.evaluate(() => window.__drags);
const emit = (page, event, payload = null) =>
  page.evaluate(([event, payload]) => window.__emit(event, payload), [event, payload]);
const library = (page, action, ...args) =>
  page.evaluate(([action, args]) => window.__library[action](...args), [action, args]);
const collectionItem = (page, label) => page.locator("#collection-list li", { hasText: label });
const outlined = (page) => page.locator(".drop-target");

// Presses at `box`'s middle-left, moves by (dx, dy) in one step, releases.
async function pullFrom(page, locator, dx, dy = 0, { offsetX = null, button = "left" } = {}) {
  const box = await locator.boundingBox();
  const x = box.x + (offsetX ?? box.width / 2);
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down({ button });
  await page.mouse.move(x + dx, y + dy);
  await page.mouse.up({ button });
}

// A drag event as Tauri reports it, at a point in CSS pixels turned into
// physical ones with `scale`.
async function dragEvent(page, type, point, paths, scale = 1) {
  const payload = { type };
  if (point) payload.position = { x: point.x * scale, y: point.y * scale };
  if (paths) payload.paths = paths;
  await page.evaluate((payload) => window.__dragDrop(payload), payload);
}

async function middleOf(locator) {
  const box = await locator.boundingBox();
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

async function createCollection(page, label) {
  await page.locator("#collection-name").fill(label);
  await page.locator("#collection-name").press("Enter");
  const item = collectionItem(page, label);
  await expect(item).toBeVisible();
  return item;
}

test.describe("the threshold", () => {
  test("a move of 4 pixels is a click, 5 pixels a drag", async ({ page }) => {
    await pullFrom(page, name(page, 2), 4, 0, { offsetX: 20 });
    expect(await drags(page)).toHaveLength(0);
    await expect(row(page, 2)).toHaveClass(/selected/);

    await pullFrom(page, name(page, 3), 5, 0, { offsetX: 20 });
    expect((await drags(page)).map((d) => d.item)).toEqual([["/lib/Drums/Kick 4.wav"]]);
  });

  test("the distance is measured diagonally: 3 across and 4 down is 5", async ({ page }) => {
    await pullFrom(page, name(page, 3), 3, 4, { offsetX: 20 });
    expect(await drags(page)).toHaveLength(1);
  });

  test("one press starts one drag, however far the mouse goes", async ({ page }) => {
    const box = await name(page, 3).boundingBox();
    await page.mouse.move(box.x + 20, box.y + 5);
    await page.mouse.down();
    await page.mouse.move(box.x + 120, box.y + 5, { steps: 10 });
    await page.mouse.up();
    expect(await drags(page)).toHaveLength(1);
  });

  test("a press released before moving leaves nothing watching", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    await name(page, 3).click();
    const box = await name(page, 3).boundingBox();
    await page.mouse.move(box.x + 200, box.y + 60, { steps: 5 });
    expect(await drags(page)).toHaveLength(0);
  });

  test("the right button never drags", async ({ page }) => {
    await pullFrom(page, name(page, 3), 30, 0, { offsetX: 20, button: "right" });
    expect(await drags(page)).toHaveLength(0);
    await page.keyboard.press("Escape");
  });
});

test.describe("where a drag starts", () => {
  test("from the ★ cell, the row is dragged and its favorite left alone", async ({ page }) => {
    await pullFrom(page, row(page, 4).locator("td.star"), 40);
    expect((await drags(page)).map((d) => d.item)).toEqual([["/lib/Drums/Kick 5.wav"]]);
    expect(await calls(page, "set_favorite")).toHaveLength(0);
    await expect(row(page, 4).locator("td.star")).toHaveText("☆");
  });

  test("from a collection's ×, the row is dragged, not taken out", async ({ page }) => {
    const item = await createCollection(page, "Set");
    await dragEvent(page, "drop", await middleOf(item), [
      "/lib/Drums/Kick 1.wav",
      "/lib/Drums/Kick 2.wav",
    ]);
    await expect(item.locator(".count")).toHaveText("2");
    await item.locator(".name").click();
    await expect(page.locator("#samples tr[data-index]")).toHaveCount(2);

    // The × ends the row, beyond the list's width at this window size.
    await row(page, 1).locator("td.remove button").scrollIntoViewIfNeeded();
    await pullFrom(page, row(page, 1).locator("td.remove button"), -40);
    expect((await drags(page)).map((d) => d.item)).toEqual([["/lib/Drums/Kick 2.wav"]]);
    expect(await calls(page, "remove_from_collection")).toHaveLength(0);
    await expect(item.locator(".count")).toHaveText("2");
  });

  test("a column handle resizes, it does not drag a sample", async ({ page }) => {
    const header = page.locator('#list-head th[data-sort="bpm"]');
    const before = (await header.boundingBox()).width;
    await pullFrom(page, header.locator(".resizer"), 40);
    expect(await drags(page)).toHaveLength(0);
    expect((await header.boundingBox()).width).toBeGreaterThan(before);
  });

  test("a press in a tempo being edited selects its text, no drag", async ({ page }) => {
    const cell = row(page, 30).locator("td.bpm");
    await cell.dblclick();
    await expect(cell.locator("input")).toBeFocused();
    await pullFrom(page, cell.locator("input"), 30);
    expect(await drags(page)).toHaveLength(0);
    await cell.locator("input").press("Escape");
  });
});

test.describe("what a drag offers", () => {
  test("a selected row takes every selected sample, those off screen too", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    await name(page, 0).click();
    await page.keyboard.press("ControlOrMeta+a");
    await expect(page.locator("#samples tr.selected").first()).toBeVisible();
    // Fewer than 50 rows are drawn: the virtual list.
    expect(await page.locator("#samples tr[data-index]").count()).toBeLessThan(50);
    await pullFrom(page, name(page, 5), 30, 0, { offsetX: 20 });
    const [drag] = await drags(page);
    expect(drag.item).toHaveLength(50);
    expect(new Set(drag.item).size).toBe(50);
  });

  test("a row outside the selection goes alone, the selection untouched", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    await name(page, 0).click();
    await name(page, 2).click({ modifiers: ["Shift"] });
    // A press and a drag only: the click that follows in Chromium (the
    // native drag swallows it in the app) is not part of the drag.
    const box = await name(page, 8).boundingBox();
    await page.mouse.move(box.x + 20, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + 50, box.y + box.height / 2);
    expect((await drags(page)).map((d) => d.item)).toEqual([["/lib/Drums/Kick 9.wav"]]);
    await expect(page.locator("#samples tr.selected")).toHaveCount(3);
    await page.mouse.up();
  });

  test("paths with accents, spaces, # and % are offered as they are", async ({ page }) => {
    const odd = [
      "/lib/Été/Grosse caisse #1 100% sèche.wav",
      "/lib/Drums/kick%20encoded.wav",
      "/lib/日本/太鼓 ドン.wav",
    ];
    for (const path of odd) await library(page, "add", path);
    await emit(page, "library-changed");
    await expect(page.locator("#status")).toHaveText("53 sample(s)");
    for (const [word, path] of [["grosse", odd[0]], ["encoded", odd[1]], ["太鼓", odd[2]]]) {
      await page.locator("#search").fill(word);
      await expect(page.locator("#status")).toHaveText("1 sample(s)");
      await pullFrom(page, name(page, 0), 30, 0, { offsetX: 20 });
      expect((await drags(page)).at(-1).item).toEqual([path]);
      expect((await drags(page)).at(-1).icon).toMatch(/^data:image\/png;base64,/);
    }

    const item = await createCollection(page, "Odd");
    await dragEvent(page, "drop", await middleOf(item), odd);
    await expect(page.locator("#status")).toHaveText("3 sample(s) added to odd.");
    expect((await calls(page, "add_to_collection")).at(-1).paths).toEqual(odd);
  });
});

for (const scale of [1.5, 2]) {
  test.describe(`a screen scaled ${scale} times`, () => {
    test.use({ deviceScaleFactor: scale });

    test("the drop lands on the collection under the cursor", async ({ page }) => {
      expect(await page.evaluate(() => window.devicePixelRatio)).toBe(scale);
      const first = await createCollection(page, "First");
      const second = await createCollection(page, "Second");
      await dragEvent(page, "over", await middleOf(second), null, scale);
      await expect(second).toHaveClass(/drop-target/);
      await expect(first).not.toHaveClass(/drop-target/);
      await dragEvent(page, "drop", await middleOf(second), ["/lib/Drums/Kick 1.wav"], scale);
      await expect(second.locator(".count")).toHaveText("1");
      await expect(first.locator(".count")).toHaveText("0");
      await expect(outlined(page)).toHaveCount(0);
    });

    test("near a collection's bottom edge, still that collection", async ({ page }) => {
      await createCollection(page, "First");
      const second = await createCollection(page, "Second");
      const box = await second.boundingBox();
      await dragEvent(
        page,
        "drop",
        { x: box.x + box.width / 2, y: box.y + box.height - 1 },
        ["/lib/Drums/Kick 1.wav"],
        scale,
      );
      await expect(second.locator(".count")).toHaveText("1");
    });
  });
}

test.describe("where a drop lands", () => {
  test("over All samples, nothing is outlined and nothing added", async ({ page }) => {
    await createCollection(page, "Set");
    const all = collectionItem(page, "All samples");
    await dragEvent(page, "over", await middleOf(all));
    await expect(outlined(page)).toHaveCount(0);
    await dragEvent(page, "drop", await middleOf(all), ["/lib/Drums/Kick 1.wav"]);
    expect(await calls(page, "add_to_collection")).toHaveLength(0);
  });

  test("on the empty space below the collections, nothing added", async ({ page }) => {
    const item = await createCollection(page, "Set");
    const list = await page.locator("#collection-list").boundingBox();
    const last = await item.boundingBox();
    const point = { x: last.x + last.width / 2, y: last.y + last.height + 10 };
    expect(point.y).toBeLessThan(list.y + list.height);
    expect(await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.closest("li"), point)).toBeNull();
    await dragEvent(page, "over", point);
    await expect(outlined(page)).toHaveCount(0);
    await dragEvent(page, "drop", point, ["/lib/Drums/Kick 1.wav"]);
    expect(await calls(page, "add_to_collection")).toHaveLength(0);
  });

  test("outside the window, nothing added and no error", async ({ page }) => {
    const item = await createCollection(page, "Set");
    await dragEvent(page, "over", await middleOf(item));
    for (const point of [
      { x: -40, y: -40 },
      { x: 99999, y: 99999 },
    ]) {
      await dragEvent(page, "over", point);
      await expect(outlined(page)).toHaveCount(0);
      await dragEvent(page, "drop", point, ["/lib/Drums/Kick 1.wav"]);
    }
    expect(await calls(page, "add_to_collection")).toHaveLength(0);
  });

  test("on a collection deleted meanwhile, nothing added and no outline left", async ({ page }) => {
    await createCollection(page, "Alpha");
    const beta = await createCollection(page, "Beta");
    const where = await middleOf(beta);
    await dragEvent(page, "over", where);
    await expect(beta).toHaveClass(/drop-target/);

    await beta.locator("button").click();
    await expect(collectionItem(page, "Beta")).toHaveCount(0);
    await dragEvent(page, "drop", where, ["/lib/Drums/Kick 1.wav"]);
    expect(await calls(page, "add_to_collection")).toHaveLength(0);
    await expect(outlined(page)).toHaveCount(0);
  });

  test("collections drawn again under the drag: the new item is the target", async ({ page }) => {
    const alpha = await createCollection(page, "Alpha");
    await dragEvent(page, "over", await middleOf(alpha));
    await expect(alpha).toHaveClass(/drop-target/);
    // Creating another one draws the list again: the outlined item is gone.
    await createCollection(page, "Zeta");
    await dragEvent(page, "drop", await middleOf(alpha), ["/lib/Drums/Kick 1.wav"]);
    await expect(alpha.locator(".count")).toHaveText("1");
    expect((await calls(page, "add_to_collection")).at(-1).collection).toBe(1);
    await expect(outlined(page)).toHaveCount(0);
  });
});

test.describe("enter, over and leave in any order", () => {
  test("a leave without an enter, then a drop without an over", async ({ page }) => {
    const item = await createCollection(page, "Set");
    await dragEvent(page, "leave");
    await expect(outlined(page)).toHaveCount(0);
    await dragEvent(page, "drop", await middleOf(item), ["/lib/Drums/Kick 1.wav"]);
    await expect(item.locator(".count")).toHaveText("1");
    await expect(outlined(page)).toHaveCount(0);
  });

  test("two enters on two collections outline the second only", async ({ page }) => {
    const first = await createCollection(page, "First");
    const second = await createCollection(page, "Second");
    await dragEvent(page, "enter", await middleOf(first), ["/x.wav"]);
    await dragEvent(page, "enter", await middleOf(second), ["/x.wav"]);
    await expect(outlined(page)).toHaveCount(1);
    await expect(second).toHaveClass(/drop-target/);
    await dragEvent(page, "leave");
    await expect(outlined(page)).toHaveCount(0);
  });

  test("a leave after the item was drawn again leaves no outline", async ({ page }) => {
    const first = await createCollection(page, "First");
    await dragEvent(page, "over", await middleOf(first));
    await createCollection(page, "Second");
    await dragEvent(page, "over", await middleOf(first));
    await expect(first).toHaveClass(/drop-target/);
    await dragEvent(page, "leave");
    await expect(outlined(page)).toHaveCount(0);
  });

  test("a drop with no file adds nothing and clears the outline", async ({ page }) => {
    const item = await createCollection(page, "Set");
    await dragEvent(page, "over", await middleOf(item));
    await dragEvent(page, "drop", await middleOf(item), []);
    await expect(outlined(page)).toHaveCount(0);
    expect(await calls(page, "add_to_collection")).toHaveLength(0);
  });
});

test.describe("what is dropped", () => {
  test("folders and files from elsewhere are counted, not added", async ({ page }) => {
    const item = await createCollection(page, "Set");
    await dragEvent(page, "drop", await middleOf(item), [
      "/lib/Drums",
      "/home/me/Desktop/new.wav",
      "/lib/Drums/Kick 1.wav",
    ]);
    await expect(page.locator("#status")).toHaveText(
      "1 sample(s) added to set (2 not in the library, left out).",
    );
    await expect(item.locator(".count")).toHaveText("1");
  });

  test("nothing from the library: none added, all counted", async ({ page }) => {
    const item = await createCollection(page, "Set");
    await dragEvent(page, "drop", await middleOf(item), ["/home/me/a.wav", "/home/me/Folder"]);
    await expect(page.locator("#status")).toHaveText(
      "0 sample(s) added to set (2 not in the library, left out).",
    );
    await expect(item.locator(".count")).toHaveText("0");
  });

  test("samples already in the collection are not added twice", async ({ page }) => {
    const item = await createCollection(page, "Set");
    await dragEvent(page, "drop", await middleOf(item), ["/lib/Drums/Kick 1.wav"]);
    await expect(item.locator(".count")).toHaveText("1");
    await dragEvent(page, "drop", await middleOf(item), [
      "/lib/Drums/Kick 1.wav",
      "/lib/Drums/Kick 1.wav",
    ]);
    await expect(page.locator("#status")).toHaveText("0 sample(s) added to set.");
    await expect(item.locator(".count")).toHaveText("1");
  });
});

test.describe("a drop while something else is open", () => {
  test("a tempo being typed is not lost", async ({ page }) => {
    const item = await createCollection(page, "Set");
    const cell = row(page, 30).locator("td.bpm");
    await cell.dblclick();
    await cell.locator("input").fill("99");
    await dragEvent(page, "drop", await middleOf(item), ["/lib/Drums/Kick 1.wav"]);
    await expect(item.locator(".count")).toHaveText("1");
    // Expected: the box stays open with its text, or the tempo is saved; the
    // refresh after the drop must not drop the edit silently.
    await expect
      .poll(async () => {
        const open = await cell.locator("input").count();
        if (open === 1) return cell.locator("input").inputValue();
        const saved = await calls(page, "set_tempo");
        return saved.some((args) => args.value === "99") ? "99" : "lost";
      })
      .toBe("99");
  });

  test("under a modal window, the collections take no drop", async ({ page }) => {
    const item = await createCollection(page, "Set");
    const where = await middleOf(item);
    await page.locator("#show-settings").click();
    await expect(page.locator("dialog[open]")).toHaveCount(1);
    await dragEvent(page, "over", where);
    await expect(outlined(page)).toHaveCount(0);
    await dragEvent(page, "drop", where, ["/lib/Drums/Kick 1.wav"]);
    expect(await calls(page, "add_to_collection")).toHaveLength(0);
    await expect(page.locator("dialog[open]")).toHaveCount(1);
    await page.keyboard.press("Escape");
  });
});

test.describe("a refresh while the button is held", () => {
  test("a change on disk during the press: the drag still takes the selection", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    await name(page, 0).click();
    await name(page, 2).click({ modifiers: ["Shift"] });
    const box = await name(page, 1).boundingBox();
    await page.mouse.move(box.x + 20, box.y + box.height / 2);
    await page.mouse.down();
    await library(page, "add", "/lib/Pads/Pad 99.wav");
    await emit(page, "library-changed");
    await expect(page.locator("#status")).toHaveText("51 sample(s)");
    await page.mouse.move(box.x + 60, box.y + box.height / 2);
    await page.mouse.up();
    expect((await drags(page)).map((d) => d.item)).toEqual([
      ["/lib/Drums/Kick 1.wav", "/lib/Drums/Kick 2.wav", "/lib/Drums/Kick 3.wav"],
    ]);
  });

  test("the pressed sample gone from the library is not offered", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    const box = await name(page, 3).boundingBox();
    await page.mouse.move(box.x + 20, box.y + box.height / 2);
    await page.mouse.down();
    await library(page, "remove", "/lib/Drums/Kick 4.wav");
    await emit(page, "library-changed");
    await expect(page.locator("#status")).toHaveText("49 sample(s)");
    await page.mouse.move(box.x + 60, box.y + box.height / 2);
    await page.mouse.up();
    // Expected: a file that left the library (deleted, renamed) is not handed to another app.
    const offered = (await drags(page)).flatMap((d) => d.item);
    expect(offered).not.toContain("/lib/Drums/Kick 4.wav");
  });
});
