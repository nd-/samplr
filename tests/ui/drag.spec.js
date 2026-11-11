// Drag and drop: rows dragged out to another app (a DAW), and rows or files
// dropped on a collection. The native drag is Tauri's: here, what the page
// asks for and how it reads what Tauri reports.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

const drags = (page) => page.evaluate(() => window.__drags);

// Presses on a row's name and moves the mouse by `distance` pixels with the
// button held, then releases it.
async function pull(page, index, distance) {
  const box = await name(page, index).boundingBox();
  const x = box.x + 20;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + distance, y, { steps: 3 });
  await page.mouse.up();
}

// Plays a drag over the window as Tauri reports it, the cursor over
// `element`'s middle (Tauri gives physical pixels).
async function dragOver(page, type, element, paths = []) {
  const box = element ? await element.boundingBox() : { x: 5, y: 5, width: 0, height: 0 };
  const scale = await page.evaluate(() => window.devicePixelRatio);
  const position = {
    x: (box.x + box.width / 2) * scale,
    y: (box.y + box.height / 2) * scale,
  };
  await page.evaluate((payload) => window.__dragDrop(payload), { type, position, paths });
}

async function createCollection(page, label) {
  await page.locator("#collection-name").fill(label);
  await page.locator("#collection-name").press("Enter");
  const item = page.locator("#collection-list li", { hasText: label });
  await expect(item).toBeVisible();
  return item;
}

test("a row starts a drag once moved 5 pixels, offered as a copy", async ({ page }) => {
  // Under 5 pixels: a click, which selects and plays.
  await pull(page, 2, 3);
  expect(await drags(page)).toHaveLength(0);
  await expect(row(page, 2)).toHaveClass(/selected/);

  await pull(page, 3, 30);
  const [drag] = await drags(page);
  expect(drag.item).toEqual(["/lib/Drums/Kick 4.wav"]);
  expect(drag.mode).toBe("copy");
  // The image under the cursor: the sample's name drawn.
  expect(drag.icon).toMatch(/^data:image\/png;base64,/);
});

test("dragging a selected row takes the selection, another row goes alone", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, 0).click();
  await name(page, 2).click({ modifiers: ["Shift"] });
  await pull(page, 1, 30);
  expect((await drags(page)).at(-1).item).toHaveLength(3);

  await pull(page, 6, 30);
  expect((await drags(page)).at(-1).item).toEqual(["/lib/Drums/Kick 7.wav"]);
});

test("the web view's own drag is cancelled", async ({ page }) => {
  const prevented = await row(page, 0).evaluate((element) => {
    const event = new DragEvent("dragstart", { bubbles: true, cancelable: true });
    element.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(prevented).toBe(true);
});

test("a collection under a drag is outlined, no longer once it leaves", async ({ page }) => {
  const item = await createCollection(page, "Set");
  await dragOver(page, "over", item);
  await expect(item).toHaveClass(/drop-target/);
  // Over another place: no collection outlined.
  await dragOver(page, "over", page.locator("#status"));
  await expect(item).not.toHaveClass(/drop-target/);
  await dragOver(page, "over", item);
  await dragOver(page, "leave", null);
  await expect(item).not.toHaveClass(/drop-target/);
});

test("files dropped on a collection join it, those from elsewhere left out", async ({ page }) => {
  const item = await createCollection(page, "Set");
  await dragOver(page, "drop", item, ["/lib/Drums/Kick 1.wav", "/home/me/Desktop/new.wav"]);
  await expect(page.locator("#status")).toHaveText(
    "1 sample(s) added to set (1 not in the library, left out).",
  );
  await expect(item.locator(".count")).toHaveText("1");
  await expect(item).not.toHaveClass(/drop-target/);
  expect((await calls(page, "add_to_collection")).at(-1)).toEqual({
    collection: 1,
    paths: ["/lib/Drums/Kick 1.wav", "/home/me/Desktop/new.wav"],
  });
});

test("a drop elsewhere than on a collection adds nothing", async ({ page }) => {
  await createCollection(page, "Set");
  await dragOver(page, "drop", page.locator("#status"), ["/lib/Drums/Kick 1.wav"]);
  await dragOver(page, "drop", page.locator("#collection-list li", { hasText: "All samples" }), [
    "/lib/Drums/Kick 1.wav",
  ]);
  expect(await calls(page, "add_to_collection")).toHaveLength(0);
});
