// The sample list at its edges: empty, a single sample, names that are not
// plain words, sorting with missing and equal values, answers coming out of
// order, refreshes over a scrolled list or a large selection, ranges over
// rows the virtual list has not drawn, and the window resized.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

const emit = (page, event, payload = null) =>
  page.evaluate(([event, payload]) => window.__emit(event, payload), [event, payload]);
const library = (page, action, ...args) =>
  page.evaluate(([action, args]) => window.__library[action](...args), [action, args]);
const settled = (page) => expect.poll(() => page.evaluate(() => window.__pending)).toBe(0);
const status = (page) => page.locator("#status");
const placeholder = (page) => page.locator("#new-tags");

async function addSamples(page, count) {
  await page.evaluate((count) => {
    for (let i = 1; i <= count; i++) window.__library.add(`/lib/Many/Hit ${i}.wav`);
  }, count);
  await emit(page, "library-changed");
  await expect(status(page)).toHaveText(`${50 + count} sample(s)`);
}

async function scrollList(page, top) {
  await page.locator("#list").evaluate((box, top) => {
    box.scrollTop = top === "end" ? box.scrollHeight : top;
  }, top);
}

test.describe("empty", () => {
  test("a search finding nothing: no row, nothing to select, and back", async ({ page }) => {
    await page.locator("#search").fill("nothing like this");
    await expect(status(page)).toHaveText("0 sample(s)");
    await expect(page.locator("#samples tr[data-index]")).toHaveCount(0);
    await expect(placeholder(page)).toBeDisabled();

    await page.locator("#list").click();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ControlOrMeta+a");
    await page.locator('th[data-sort="bpm"]').click();
    await expect(page.locator("#samples tr.selected")).toHaveCount(0);
    expect(await calls(page, "play")).toHaveLength(0);
    await expect(page.locator("#now-name")).toHaveText("");

    await page.locator("#search").fill("");
    await expect(status(page)).toHaveText("50 sample(s)");
    await expect(row(page, 0)).toBeVisible();
  });

  test("the whole library gone, then a sample coming back", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    await name(page, 3).click();
    // Every sample hidden rather than removed: the fake makes a new sample
    // from its first one.
    await page.evaluate(() => window.__TAURI__.core.invoke("remove_folder", { folder: "/lib" }));
    await emit(page, "library-changed");
    await expect(status(page)).toHaveText("0 sample(s)");
    await expect(page.locator("#samples tr[data-index]")).toHaveCount(0);
    await expect(placeholder(page)).toBeDisabled();
    await expect(page.locator("#now-name")).toHaveText("");

    await library(page, "add", "/elsewhere/Back.wav");
    await emit(page, "library-changed");
    await expect(status(page)).toHaveText("1 sample(s)");
    await expect(name(page, 0)).toHaveText("Back.wav");
  });

  test("a library empty at startup, filled later: rows measured then", async ({ page }) => {
    // The page measures a row's height on the first row it draws: here none
    // at startup.
    await page.addInitScript(() => {
      const real = window.__TAURI__.core.invoke;
      window.__emptyLibrary = true;
      window.__TAURI__.core.invoke = (command, args) =>
        command === "search" && window.__emptyLibrary ? Promise.resolve([]) : real(command, args);
    });
    await page.reload();
    await expect(status(page)).toHaveText("0 sample(s)");
    await settled(page);

    await page.evaluate(() => {
      window.__emptyLibrary = false;
    });
    await emit(page, "library-changed");
    await expect(status(page)).toHaveText("50 sample(s)");
    const height = await row(page, 0).evaluate((tr) => tr.getBoundingClientRect().height);
    // The spacers' height comes from the row measured: the list is exactly
    // 50 rows high, and the last one is reached.
    const content = await page.locator("#samples").evaluate((body) => body.getBoundingClientRect().height);
    expect(Math.abs(content - 50 * height)).toBeLessThan(1);
    await scrollList(page, "end");
    await expect(row(page, 49)).toBeVisible();
  });
});

test("a single sample: the arrows stay on it, sorting keeps it", async ({ page }) => {
  await page.locator("#search").fill("Kick 12");
  await expect(status(page)).toHaveText("1 sample(s)");
  await name(page, 0).click();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Shift+ArrowDown");
  await page.locator('th[data-sort="name"]').click();
  await page.locator('th[data-sort="name"]').click();
  await expect(row(page, 0)).toHaveClass(/selected/);
  await expect(page.locator("#now-name")).toHaveText("Kick 12.wav");
  // The click only: no other sample to move to.
  expect(await calls(page, "play")).toHaveLength(1);
});

test.describe("names", () => {
  const odd = [
    "/lib/Ünïcödé/Été à Paris — 東京 サンプル.wav",
    "/lib/Emoji/🥁 Kick 🔥🎹.wav",
    `/lib/Long/${"Aaaaaaaaaa".repeat(40)}.wav`,
    "/lib/Html/<b>x<i>.wav",
    "/lib/Html/&amp; &lt;i&gt;.wav",
    "/lib/<img src=x onerror=window.__injected=1>/Kick.wav",
  ];

  test.beforeEach(async ({ page }) => {
    for (const path of odd) await library(page, "add", path);
    await library(page, "tag", "/lib/Html/<b>x<i>.wav", "<i>tag</i>");
    await emit(page, "library-changed");
    await expect(status(page)).toHaveText(`${50 + odd.length} sample(s)`);
  });

  test("shown as text, never as markup", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    await scrollList(page, "end");
    for (let i = 0; i < odd.length; i++) {
      const path = odd[i];
      const index = 50 + i;
      await expect(name(page, index)).toHaveText(path.slice(path.lastIndexOf("/") + 1));
      await expect(row(page, index)).toHaveAttribute("title", path);
    }
    await expect(row(page, 53).locator("td.tags")).toHaveText("<i>tag</i>");
    await expect(row(page, 55).locator("td.folder")).toHaveText("/lib/<img src=x onerror=window.__injected=1>");
    await name(page, 53).click();
    await expect(page.locator("#now-name")).toHaveText("<b>x<i>.wav");
    await expect(page.locator("#tagbar .tag")).toHaveText(["<i>tag</i>×"]);
    await expect(page.locator("#info-list")).toContainText("<b>x<i>.wav");
    // Nothing the names hold became an element.
    for (const selector of ["#samples b", "#samples i", "#samples img", "#now-name b", "#info-list b", "#tagbar i"]) {
      await expect(page.locator(selector)).toHaveCount(0);
    }
    expect(await page.evaluate(() => window.__injected)).toBeUndefined();
  });

  test("a long name, emoji or another script keep the row's height and the table's width", async ({ page }) => {
    const height = await row(page, 0).evaluate((tr) => tr.getBoundingClientRect().height);
    const width = await page.locator("#list table").evaluate((table) => table.getBoundingClientRect().width);
    await scrollList(page, "end");
    for (const index of [50, 51, 52]) {
      const box = await row(page, index).boundingBox();
      expect(box.height).toBe(height);
    }
    expect(await page.locator("#list table").evaluate((table) => table.getBoundingClientRect().width)).toBe(width);
    // The long name ends with an ellipsis inside its cell.
    const cell = name(page, 52);
    expect(await cell.evaluate((td) => td.scrollWidth > td.clientWidth)).toBe(true);
    expect(await cell.evaluate((td) => getComputedStyle(td).textOverflow)).toBe("ellipsis");
  });

  test("a Windows path: name and folder split on the backslash", async ({ page }) => {
    await library(page, "add", "C:\\Samples\\Drums\\Win Kick.wav");
    await emit(page, "library-changed");
    await page.locator("#search").fill("samples");
    await expect(status(page)).toHaveText("1 sample(s)");
    await expect(name(page, 0)).toHaveText("Win Kick.wav");
    await expect(row(page, 0).locator("td.folder")).toHaveText("C:\\Samples\\Drums");
  });
});

test.describe("sorting", () => {
  const header = (page, key) => page.locator(`th[data-sort="${key}"]`);
  const names = (page, from, to) =>
    page.evaluate(
      ([from, to]) =>
        [...document.querySelectorAll("#samples tr[data-index]")]
          .filter((tr) => Number(tr.dataset.index) >= from && Number(tr.dataset.index) <= to)
          .map((tr) => tr.children[1].textContent),
      [from, to],
    );

  test("missing lengths and loudness stay last in both directions", async ({ page }) => {
    await library(page, "set", "/lib/Drums/Kick 3.wav", { duration_secs: null, lufs: null });
    await library(page, "set", "/lib/Pads/Pad 1.wav", { duration_secs: null, lufs: null });
    await emit(page, "library-changed");
    await expect(row(page, 2).locator("td.number").first()).toHaveText("?");

    for (const key of ["length", "lufs"]) {
      await header(page, key).click();
      await scrollList(page, "end");
      // Among themselves, the missing ones by path.
      await expect.poll(() => names(page, 48, 49)).toEqual(["Kick 3.wav", "Pad 1.wav"]);
      await header(page, key).click();
      await scrollList(page, "end");
      await expect.poll(() => names(page, 48, 49)).toEqual(["Kick 3.wav", "Pad 1.wav"]);
      await scrollList(page, 0);
      await expect(name(page, 0)).not.toHaveText(/Kick 3|Pad 1/);
    }
  });

  test("equal values fall back to the path, in natural order, both directions", async ({ page }) => {
    // Every loop has 120 BPM: the 13 basses first either way, Bass 2 before
    // Bass 10.
    const basses = Array.from({ length: 13 }, (_, i) => `Bass ${i + 1} 120BPM Am.wav`);
    await header(page, "bpm").click();
    await expect.poll(() => names(page, 0, 12)).toEqual(basses);
    await header(page, "bpm").click();
    await expect(header(page, "bpm")).toHaveClass(/sorted-descending/);
    await expect.poll(() => names(page, 0, 12)).toEqual(basses);
  });

  test("numbers in names in their order, case ignored", async ({ page }) => {
    await library(page, "add", "/lib/Drums/kick 3a.wav");
    await library(page, "add", "/lib/Drums/Kick 007.wav");
    await emit(page, "library-changed");
    await page.locator("#search").fill("kick");
    await expect(status(page)).toHaveText("14 sample(s)");
    await header(page, "name").click();
    await expect
      .poll(() => names(page, 0, 13))
      .toEqual([
        "Kick 1.wav",
        "Kick 2.wav",
        "Kick 3.wav",
        "kick 3a.wav",
        "Kick 4.wav",
        "Kick 5.wav",
        "Kick 6.wav",
        "Kick 7.wav",
        "Kick 007.wav",
        "Kick 8.wav",
        "Kick 9.wav",
        "Kick 10.wav",
        "Kick 11.wav",
        "Kick 12.wav",
      ]);
  });

  test("the sort is kept by a refresh, and a sample added takes its place", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    await header(page, "name").click();
    await name(page, 5).click();
    const current = await name(page, 5).textContent();
    await library(page, "add", "/lib/Zz/Aaa first.wav");
    await emit(page, "library-changed");
    await expect(status(page)).toHaveText("51 sample(s)");
    await expect(name(page, 0)).toHaveText("Aaa first.wav");
    // The selected sample moved down one row with its selection.
    await expect(row(page, 6)).toHaveClass(/selected/);
    await expect(name(page, 6)).toHaveText(current);
    await expect(page.locator("#now-name")).toHaveText(current);
  });
});

test.describe("answers out of order", () => {
  test("a slow search answer does not replace a refresh made after it", async ({ page }) => {
    // The search for "kick" is answered late, with the library as it was
    // when asked: twelve kicks. A thirteenth comes meanwhile.
    await page.evaluate(() => {
      let slow = true;
      window.__delay = (command, args) => {
        if (command === "search" && args.query === "kick" && slow) {
          slow = false;
          return 600;
        }
        return 0;
      };
    });
    await page.locator("#search").fill("kick");
    await expect.poll(async () => (await calls(page, "search")).at(-1).query).toBe("kick");
    await library(page, "add", "/lib/Drums/Kick 13.wav");
    await emit(page, "library-changed");
    await expect(status(page)).toHaveText("13 sample(s)");
    await settled(page);
    await expect(status(page)).toHaveText("13 sample(s)");
  });

  test("a slow refresh does not replace a search typed after it", async ({ page }) => {
    await page.evaluate(() => {
      let slow = true;
      window.__delay = (command) => {
        if (command === "search" && slow) {
          slow = false;
          return 600;
        }
        return 0;
      };
    });
    await library(page, "remove", "/lib/Pads/Pad 13.wav");
    await emit(page, "library-changed");
    await page.locator("#search").fill("snare");
    await expect(status(page)).toHaveText("12 sample(s)");
    await settled(page);
    await expect(status(page)).toHaveText("12 sample(s)");
    await expect(name(page, 0)).toHaveText("Snare 1.wav");
  });

  test("a row clicked while a refresh is on its way stays the current one", async ({ page }) => {
    // Expected: the refresh keeps the sample selected when its answer comes
    // (Kick 6), not the one selected when it was asked (Kick 3).
    await page.locator("#autoplay").uncheck();
    await name(page, 2).click();
    await page.evaluate(() => {
      window.__delay = (command) => (command === "search" ? 400 : 0);
    });
    await emit(page, "library-changed");
    await name(page, 5).click();
    await expect(page.locator("#now-name")).toHaveText("Kick 6.wav");
    await settled(page);
    await expect(row(page, 5)).toHaveClass(/selected/);
    await expect(page.locator("#now-name")).toHaveText("Kick 6.wav");
    // The arrow moves on from the row clicked.
    await page.keyboard.press("ArrowDown");
    await expect(page.locator("#now-name")).toHaveText("Kick 7.wav");
  });
});

test.describe("refreshing a large list", () => {
  test("scrolled far down: the scroll and the selection kept", async ({ page }) => {
    await addSamples(page, 950);
    await page.locator("#autoplay").uncheck();
    await scrollList(page, 20000);
    const firstDrawn = () => page.locator("#samples tr[data-index]").first().getAttribute("data-index");
    await expect.poll(firstDrawn).not.toBe("0");
    const first = Number(await firstDrawn());
    // Past the overscan, and far enough from the top to stay on screen once
    // five rows above are gone.
    const index = first + 25;
    await name(page, index).click();
    const current = await name(page, index).textContent();
    const scrolled = await page.locator("#list").evaluate((box) => box.scrollTop);

    // Samples removed above and added below the rows on screen.
    for (let i = 1; i <= 5; i++) await library(page, "remove", `/lib/Drums/Kick ${i}.wav`);
    await library(page, "add", "/lib/Zz/New.wav");
    await emit(page, "library-changed");
    await expect(status(page)).toHaveText("996 sample(s)");
    expect(await page.locator("#list").evaluate((box) => box.scrollTop)).toBe(scrolled);
    await expect(page.locator("#samples tr.selected")).toHaveCount(1);
    await expect(row(page, index - 5)).toHaveClass(/selected/);
    await expect(name(page, index - 5)).toHaveText(current);
    await expect(row(page, index - 5)).toBeInViewport();
  });

  test("scrolled to the end, the list shrinking under it: rows still drawn", async ({ page }) => {
    await addSamples(page, 450);
    await scrollList(page, "end");
    await expect(row(page, 499)).toBeVisible();
    for (let i = 101; i <= 450; i++) await library(page, "remove", `/lib/Many/Hit ${i}.wav`);
    await emit(page, "library-changed");
    await expect(status(page)).toHaveText("150 sample(s)");
    await expect(row(page, 149)).toBeInViewport();
  });

  test("Ctrl+A, then a refresh taking some selected samples away", async ({ page }) => {
    await addSamples(page, 950);
    await page.locator("#list").click();
    await page.keyboard.press("ControlOrMeta+a");
    await expect(placeholder(page)).toHaveAttribute("placeholder", "Add tags to 1000 samples");

    // The current row (the first) among them, and some off screen.
    const gone = ["/lib/Drums/Kick 1.wav", "/lib/Pads/Pad 2.wav", "/lib/Many/Hit 10.wav", "/lib/Many/Hit 900.wav"];
    for (const path of gone) await library(page, "remove", path);
    await emit(page, "library-changed");
    await expect(status(page)).toHaveText("996 sample(s)");
    await expect(placeholder(page)).toHaveAttribute("placeholder", "Add tags to 996 samples");
    await expect(row(page, 0)).toHaveClass(/selected/);

    // Tagging reaches every sample left, none of those gone (the fake
    // refuses a path not in the library).
    await page.keyboard.press("ControlOrMeta+t");
    await page.keyboard.type("bulk");
    await page.keyboard.press("Enter");
    await expect(page.locator("#tagbar .tag")).toHaveText(["bulk×"]);
    const paths = (await calls(page, "add_tags")).at(-1).paths;
    expect(paths).toHaveLength(996);
    expect(paths.filter((path) => gone.includes(path))).toEqual([]);
  });

  test("Shift+click from a row to one far below, over rows not drawn", async ({ page }) => {
    await addSamples(page, 450);
    await page.locator("#autoplay").uncheck();
    await name(page, 3).click();
    await scrollList(page, "end");
    await name(page, 490).click({ modifiers: ["Shift"] });
    await expect(placeholder(page)).toHaveAttribute("placeholder", "Add tags to 488 samples");
    await expect(row(page, 491)).not.toHaveClass(/selected/);

    // The rows drawn on the way back are selected too.
    await scrollList(page, 250 * 26);
    await expect(row(page, 250)).toHaveClass(/selected/);
    await scrollList(page, 0);
    await expect(row(page, 3)).toHaveClass(/selected/);
    await expect(row(page, 2)).not.toHaveClass(/selected/);

    // And back: Shift+click on the anchor's side narrows the range.
    await name(page, 10).click({ modifiers: ["Shift"] });
    await expect(placeholder(page)).toHaveAttribute("placeholder", "Add tags to 8 samples");
  });
});

test("the selected sample removed: nothing current, the arrows start again", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, 7).click();
  await expect(page.locator("#now-name")).toHaveText("Kick 8.wav");
  await library(page, "remove", "/lib/Drums/Kick 8.wav");
  await emit(page, "library-changed");
  await expect(status(page)).toHaveText("49 sample(s)");
  await expect(page.locator("#samples tr.selected")).toHaveCount(0);
  await expect(page.locator("#now-name")).toHaveText("");
  await expect(placeholder(page)).toBeDisabled();
  expect(await calls(page, "stop")).toHaveLength(0);

  await page.keyboard.press("ArrowDown");
  await expect(row(page, 0)).toHaveClass(/selected/);
  await expect(page.locator("#now-name")).toHaveText("Kick 1.wav");
});

test("the current sample of a selection removed: the others stay selected", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, 4).click();
  await name(page, 6).click({ modifiers: ["Shift"] });
  await library(page, "remove", "/lib/Drums/Kick 7.wav");
  await emit(page, "library-changed");
  await expect(status(page)).toHaveText("49 sample(s)");
  await expect(page.locator("#samples tr.selected")).toHaveCount(2);
  await expect(placeholder(page)).toHaveAttribute("placeholder", "Add tags to 2 samples");
  await expect(page.locator("#samples tr.current")).toHaveCount(0);
});

test.describe("the window resized", () => {
  test("taller: the rows coming into view are drawn", async ({ page }) => {
    await addSamples(page, 150);
    await scrollList(page, 100 * 26);
    await expect(row(page, 100)).toBeInViewport();
    await page.setViewportSize({ width: 1400, height: 2400 });
    // The bottom of the list shows a row, not an empty spacer.
    const atBottom = () =>
      page.evaluate(() => {
        const list = document.getElementById("list").getBoundingClientRect();
        return document.elementFromPoint(list.left + 200, list.bottom - 5)?.closest("tr")?.dataset.index ?? null;
      });
    await expect.poll(atBottom).not.toBeNull();
    await expect(row(page, Number(await atBottom()))).toBeInViewport();
  });

  test("narrower: the list scrolls sideways, header and rows aligned", async ({ page }) => {
    await page.setViewportSize({ width: 700, height: 600 });
    const list = page.locator("#list");
    await expect.poll(() => list.evaluate((box) => box.scrollWidth > box.clientWidth)).toBe(true);
    const widths = await page.evaluate(() =>
      [...document.querySelectorAll("#list-head table, #list table")].map((t) => t.getBoundingClientRect().width),
    );
    expect(widths[0]).toBe(widths[1]);

    await list.evaluate((box) => {
      box.scrollLeft = 300;
    });
    await expect.poll(() => page.locator("#list-head").evaluate((head) => head.scrollLeft)).toBe(300);
    const header = await page.locator('th[data-sort="bpm"]').boundingBox();
    const cell = await row(page, 0).locator("td.bpm").boundingBox();
    expect(Math.abs(header.x - cell.x)).toBeLessThan(1);
    expect(Math.abs(header.width - cell.width)).toBeLessThan(1);

    // Wide again: no sideways scroll left.
    await page.setViewportSize({ width: 2600, height: 800 });
    await expect.poll(() => list.evaluate((box) => box.scrollWidth <= box.clientWidth)).toBe(true);
  });
});
