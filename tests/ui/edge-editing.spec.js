// Editing in place while the list is drawn again: changes on disk
// (`library-changed`) during a tempo or key edit, the type picker, the
// rename box, a colour name and the tag box; the refresh applied once the
// edit ends; slow answers; scrolling the row away; values at the limits.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

// In the fake library: Kick 1 (no tempo), Snare 1 (no type), Bass 7 and
// Bass 8 (120 BPM, Am), and Pad 13, the last row, removed on disk below.
const KICK = 0;
const SNARE = 12;
const BASS = 30;
const PAD_13 = "/lib/Pads/Pad 13.wav";

const emit = (page, event) => page.evaluate((event) => window.__emit(event, null), event);
const library = (page, action, ...args) =>
  page.evaluate(([action, args]) => window.__library[action](...args), [action, args]);
const searches = async (page) => (await calls(page, "search")).length;
const settled = (page) => expect.poll(() => page.evaluate(() => window.__pending)).toBe(0);
const bpmCell = (page, index) => row(page, index).locator("td.bpm");
const keyCell = (page, index) => row(page, index).locator("td.key");
const typeCell = (page, index) => row(page, index).locator("td.type");
const delay = (page, command, ms) =>
  page.evaluate(([command, ms]) => {
    window.__delay = (c) => (c === command ? ms : 0);
  }, [command, ms]);

// Pad 13 deleted on disk, reported `times` times in a row. The listener
// runs at once: a refresh not waiting would have asked `search` already.
async function padDeleted(page, times = 1) {
  await library(page, "remove", PAD_13);
  for (let i = 0; i < times; i++) await emit(page, "library-changed");
}

async function editTempo(page, index, typed) {
  await bpmCell(page, index).dblclick();
  const box = bpmCell(page, index).locator("input");
  await expect(box).toBeFocused();
  await box.fill(typed);
  return box;
}

// The refresh that waited came, once: Pad 13 is gone.
async function refreshedOnce(page, before) {
  await expect(page.locator("#status")).toHaveText("49 sample(s)");
  await settled(page);
  expect(await searches(page)).toBe(before + 1);
}

test.describe("a change on disk while a tempo is edited", () => {
  test("waits, and comes once after Esc, the old value back", async ({ page }) => {
    const box = await editTempo(page, BASS, "99");
    const before = await searches(page);
    await padDeleted(page, 3);
    expect(await searches(page)).toBe(before);
    await expect(box).toBeFocused();
    await expect(box).toHaveValue("99");
    await expect(page.locator("#status")).toHaveText("50 sample(s)");

    await box.press("Escape");
    await expect(bpmCell(page, BASS)).toHaveText("120");
    await refreshedOnce(page, before);
    expect(await calls(page, "set_tempo")).toHaveLength(0);
  });

  test("waits, and comes once after Enter, with the new value", async ({ page }) => {
    const box = await editTempo(page, BASS, "99");
    const before = await searches(page);
    await padDeleted(page, 3);
    await box.press("Enter");
    await expect(bpmCell(page, BASS)).toHaveText("99");
    await refreshedOnce(page, before);
  });

  test("waits, and comes once after leaving the box", async ({ page }) => {
    const box = await editTempo(page, BASS, "99");
    const before = await searches(page);
    await padDeleted(page, 2);
    await box.blur();
    await expect(bpmCell(page, BASS)).toHaveText("99");
    await refreshedOnce(page, before);
  });

  test("Enter on the same value saves nothing and still refreshes", async ({ page }) => {
    const box = await editTempo(page, BASS, " 120 ");
    const before = await searches(page);
    await padDeleted(page);
    await box.press("Enter");
    await expect(bpmCell(page, BASS)).toHaveText("120");
    await refreshedOnce(page, before);
    expect(await calls(page, "set_tempo")).toHaveLength(0);
  });

  test("a value refused, then Esc: the old value, the refresh once", async ({ page }) => {
    const box = await editTempo(page, BASS, "abc");
    await box.press("Enter");
    await expect(box).toHaveClass(/invalid/);
    const before = await searches(page);
    await padDeleted(page, 2);
    expect(await searches(page)).toBe(before);
    await expect(box).toBeFocused();

    await box.press("Escape");
    await expect(bpmCell(page, BASS)).toHaveText("120");
    await refreshedOnce(page, before);
    await page.locator("#error-close").click();
  });

  test("a value refused on leaving the box: the old value, the refresh once", async ({ page }) => {
    const box = await editTempo(page, BASS, "abc");
    const before = await searches(page);
    await padDeleted(page);
    await box.blur();
    await expect(page.locator("#error-text")).toContainText("is not a tempo");
    await expect(bpmCell(page, BASS)).toHaveText("120");
    await refreshedOnce(page, before);
    await page.locator("#error-close").click();
  });

  test("the sample edited deleted meanwhile: refused, and gone after Esc", async ({ page }) => {
    const box = await editTempo(page, BASS, "99");
    await library(page, "remove", "/lib/Loops/Bass 7 120BPM Am.wav");
    await emit(page, "library-changed");
    await box.press("Enter");
    await expect(page.locator("#error-text")).toContainText("is not in the database");
    await expect(box).toHaveClass(/invalid/);
    await box.press("Escape");
    await expect(page.locator("#status")).toHaveText("49 sample(s)");
    await expect(name(page, BASS)).toHaveText("Bass 8 120BPM Am.wav");
    await page.locator("#error-close").click();
  });

  test("an estimated tempo comes back as an estimate after Esc", async ({ page }) => {
    await library(page, "set", "/lib/Drums/Kick 1.wav", { bpm: 100, bpm_source: "length" });
    await emit(page, "library-changed");
    await expect(bpmCell(page, KICK)).toHaveText("≈ 100");
    const box = await editTempo(page, KICK, "130");
    await expect(box).toHaveValue("130");
    await box.press("Escape");
    await expect(bpmCell(page, KICK)).toHaveText("≈ 100");
    await expect(bpmCell(page, KICK)).toHaveClass(/estimated/);
  });
});

test.describe("a slow answer while editing", () => {
  test("a change on disk during a slow save: one refresh shows both", async ({ page }) => {
    await delay(page, "set_tempo", 300);
    const box = await editTempo(page, BASS, "99");
    const before = await searches(page);
    await box.press("Enter");
    await padDeleted(page, 2);
    await expect(bpmCell(page, BASS)).toHaveText("99");
    await refreshedOnce(page, before);
  });

  test("a second cell edited while the first one's save is pending stays open", async ({
    page,
  }) => {
    await delay(page, "set_tempo", 300);
    await editTempo(page, BASS, "99");
    // Leaving the first box for the second one saves the first.
    await bpmCell(page, BASS + 1).dblclick();
    const second = bpmCell(page, BASS + 1).locator("input");
    await expect(second).toBeFocused();
    await expect.poll(() => calls(page, "set_tempo")).toHaveLength(1);
    await settled(page);
    // Expected: the refresh after the first save waits for the second edit.
    await expect(second).toBeVisible();
    await second.fill("98");
    await second.press("Enter");
    await expect(bpmCell(page, BASS)).toHaveText("99");
    await expect(bpmCell(page, BASS + 1)).toHaveText("98");
  });
});

test.describe("a change on disk while another box is open", () => {
  test("the type picker: waits, then one refresh after Esc", async ({ page }) => {
    await typeCell(page, SNARE).dblclick();
    await expect(page.locator("#type-picker")).toBeVisible();
    const before = await searches(page);
    await padDeleted(page, 3);
    expect(await searches(page)).toBe(before);
    await expect(page.locator("#type-search")).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(page.locator("#type-picker")).toBeHidden();
    await refreshedOnce(page, before);
    expect(await calls(page, "set_sound_type")).toHaveLength(0);
  });

  test("the type picker: a type chosen after a change shows with it", async ({ page }) => {
    await typeCell(page, SNARE).dblclick();
    await padDeleted(page);
    await page.locator("#type-subtypes button", { hasText: "Snare" }).click();
    await expect(typeCell(page, SNARE)).toHaveText("Drums › Snare");
    await expect(page.locator("#status")).toHaveText("49 sample(s)");
    await settled(page);
    await expect(typeCell(page, SNARE)).toHaveText("Drums › Snare");
  });

  test("the rename box keeps its text and renames the right file", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    await name(page, KICK).click({ button: "right" });
    await page.locator("#context-menu button", { hasText: "Rename file…" }).click();
    const box = page.locator("#context-menu input[title]");
    await box.fill("Kick dry");
    await padDeleted(page);
    await expect(page.locator("#status")).toHaveText("49 sample(s)");
    await expect(box).toBeFocused();
    await expect(box).toHaveValue("Kick dry");

    await box.press("Enter");
    await expect(name(page, KICK)).toHaveText("Kick dry.wav");
    expect(await calls(page, "rename_sample")).toEqual([
      { path: "/lib/Drums/Kick 1.wav", name: "Kick dry" },
    ]);
  });

  test("a colour name being edited keeps its text, and Enter saves it", async ({ page }) => {
    await page.locator("#show-settings").click();
    const first = page.locator("#color-list li").nth(0);
    await first.locator("button", { hasText: "✎" }).click();
    const box = first.locator('input[type="text"]');
    await box.fill("Brick");
    await padDeleted(page);
    await expect(page.locator("#status")).toHaveText("49 sample(s)");
    await expect(box).toBeFocused();
    await expect(box).toHaveValue("Brick");
    await expect(box).not.toHaveAttribute("readonly");

    await box.press("Enter");
    await expect.poll(async () => (await calls(page, "update_color")).at(-1)).toEqual({
      id: 1,
      name: "Brick",
      hex: "#e5484d",
    });
  });

  test("a colour name: Esc after a change on disk puts the old one back", async ({ page }) => {
    await page.locator("#show-settings").click();
    const first = page.locator("#color-list li").nth(0);
    await first.locator("button", { hasText: "✎" }).click();
    const box = first.locator('input[type="text"]');
    await box.fill("Brick");
    await padDeleted(page);
    await expect(page.locator("#status")).toHaveText("49 sample(s)");
    await box.press("Escape");
    await expect(box).toHaveValue("Red");
    await expect(box).toHaveAttribute("readonly");
    expect(await calls(page, "update_color")).toHaveLength(0);
  });

  test("the tag box keeps what is typed, for the sample selected", async ({ page }) => {
    await page.locator("#autoplay").uncheck();
    await name(page, KICK).click();
    await page.keyboard.press("ControlOrMeta+t");
    await page.keyboard.type("dusty");
    await padDeleted(page);
    await expect(page.locator("#status")).toHaveText("49 sample(s)");
    const input = page.locator("#new-tags");
    await expect(input).toBeFocused();
    await expect(input).toHaveValue("dusty");

    await page.keyboard.press("Enter");
    expect((await calls(page, "add_tags")).at(-1)).toEqual({
      paths: ["/lib/Drums/Kick 1.wav"],
      tags: ["dusty"],
    });
    await expect(row(page, KICK).locator("td.tags")).toHaveText("dusty");
  });
});

test("a double-click on a selection, a change on disk between the clicks", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, BASS).click();
  await name(page, BASS + 2).click({ modifiers: ["Shift"] });
  await expect(page.locator("#samples tr.selected")).toHaveCount(3);

  const box = await bpmCell(page, BASS + 1).boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down({ clickCount: 1 });
  await page.mouse.up({ clickCount: 1 });
  await padDeleted(page);
  await expect(page.locator("#status")).toHaveText("49 sample(s)");
  await page.mouse.down({ clickCount: 2 });
  await page.mouse.up({ clickCount: 2 });

  const input = bpmCell(page, BASS + 1).locator("input");
  await expect(input).toBeFocused();
  await input.fill("99");
  await input.press("Enter");
  expect((await calls(page, "set_tempo")).at(-1).paths).toEqual([
    "/lib/Loops/Bass 7 120BPM Am.wav",
    "/lib/Loops/Bass 8 120BPM Am.wav",
    "/lib/Loops/Bass 9 120BPM Am.wav",
  ]);
  await expect(page.locator("#samples tr.selected")).toHaveCount(3);
  for (const index of [BASS, BASS + 1, BASS + 2]) {
    await expect(bpmCell(page, index)).toHaveText("99");
  }
});

test.describe("the edited row scrolled out of the list", () => {
  const scrollToEnd = (page) =>
    page.locator("#list").evaluate((list) => {
      list.scrollTop = list.scrollHeight;
    });

  // Chromium sends `blur` to a focused box taken out of the page, which
  // saves what it holds; WebKit, the app's engine, does not.
  test("Chromium: the box taken away saves its value, and the refresh comes", async ({
    page,
  }) => {
    await editTempo(page, KICK, "99");
    await padDeleted(page);
    await scrollToEnd(page);
    await expect(row(page, KICK)).toHaveCount(0);
    await expect(page.locator("#status")).toHaveText("49 sample(s)");
    expect(await calls(page, "set_tempo")).toEqual([
      { paths: ["/lib/Drums/Kick 1.wav"], value: "99" },
    ]);
  });

  // WebKit sends no `blur` for a box taken out of the page: the page must
  // leave the box itself, while it is still there, whatever the engine.
  test("the box is left before its row goes, not by the engine's removal", async ({ page }) => {
    await editTempo(page, KICK, "99");
    await page.evaluate(() => {
      window.__blurWhileConnected = [];
      document.querySelector("#list tbody input").addEventListener("blur", (event) => {
        window.__blurWhileConnected.push(event.target.isConnected);
      });
    });
    await padDeleted(page);
    await scrollToEnd(page);
    await expect(row(page, KICK)).toHaveCount(0);
    expect(await page.evaluate(() => window.__blurWhileConnected)).toEqual([true]);
    await expect(page.locator("#status")).toHaveText("49 sample(s)");
  });

  test("scrolled back, the row shows no box, and a change on disk applies at once", async ({
    page,
  }) => {
    await editTempo(page, KICK, "99");
    await scrollToEnd(page);
    await expect(row(page, KICK)).toHaveCount(0);
    await page.locator("#list").evaluate((list) => {
      list.scrollTop = 0;
    });
    await expect(bpmCell(page, KICK).locator("input")).toHaveCount(0);
    const before = await searches(page);
    await padDeleted(page);
    await refreshedOnce(page, before);
  });
});

// The tempo is checked on the Rust side (`tempo_value`, an integer parsed
// as `u32`, 20 to 400): the fake accepts what JavaScript's `Number` reads
// ("1e2", "120.0"), so its answers are made Rust's here.
test.describe("tempos at the limits", () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      const fakeInvoke = window.__TAURI__.core.invoke;
      window.__TAURI__.core.invoke = (command, args) => {
        if (command === "set_tempo") {
          const text = args.value.trim();
          const bpm = /^\+?[0-9]+$/.test(text) ? Number(text) : NaN;
          if (!["", "-", "auto"].includes(text) && !(bpm >= 20 && bpm <= 400)) {
            window.__calls.push({ command, args: structuredClone(args) });
            return Promise.reject(`“${text}” is not a tempo between 20 and 400`);
          }
        }
        return fakeInvoke(command, args);
      };
    });
    await page.reload();
    await expect(page.locator("#status")).toHaveText("50 sample(s)");
    await settled(page);
  });

  // What is typed, what is sent, what the cell then shows (`null`: refused).
  const cases = [
    ["20", "20", "20"],
    ["400", "400", "400"],
    ["19", "19", null],
    ["401", "401", null],
    ["19.9", "19.9", null],
    ["400.1", "400.1", null],
    ["  99  ", "99", "99"],
    ["+120", "+120", "120"],
    ["0120", "0120", "120"],
    ["١٢٠", "١٢٠", null],
    ["120bpm", "120bpm", null],
    ["1e2", "1e2", null],
    ["-", "-", ""],
  ];
  for (const [typed, sent, shown] of cases) {
    test(`"${typed}"`, async ({ page }) => {
      const box = await editTempo(page, BASS, typed);
      await box.press("Enter");
      expect((await calls(page, "set_tempo")).at(-1)).toEqual({
        paths: ["/lib/Loops/Bass 7 120BPM Am.wav"],
        value: sent,
      });
      if (shown === null) {
        await expect(box).toHaveClass(/invalid/);
        await expect(box).toHaveAttribute("title", /is not a tempo between 20 and 400/);
        await box.press("Escape");
        await expect(bpmCell(page, BASS)).toHaveText("120");
        await page.locator("#error-close").click();
      } else {
        await expect(bpmCell(page, BASS)).toHaveText(shown);
      }
    });
  }

  test("empty or blank: none", async ({ page }) => {
    for (const typed of ["", "   "]) {
      const box = await editTempo(page, BASS, typed);
      await box.press("Enter");
      await expect(bpmCell(page, BASS)).toHaveText("");
      expect((await calls(page, "set_tempo")).at(-1).value).toBe("");
      await library(page, "set", "/lib/Loops/Bass 7 120BPM Am.wav", {
        bpm: 120,
        bpm_source: "name",
      });
      await emit(page, "library-changed");
      await expect(bpmCell(page, BASS)).toHaveText("120");
    }
  });
});

// Spellings Rust's `normalize_key` and the fake read alike.
test.describe("keys in any spelling", () => {
  const cases = [
    ["f#m", "F#m"],
    ["  bbm ", "A#m"],
    ["Gb", "F#"],
    ["cmaj", "C"],
    ["E#", "F"],
    ["Cb", "B"],
    ["dMinor", "Dm"],
    ["-", ""],
  ];
  for (const [typed, shown] of cases) {
    test(`"${typed}" is ${shown || "none"}`, async ({ page }) => {
      await keyCell(page, KICK).dblclick();
      const box = keyCell(page, KICK).locator("input");
      await box.fill(typed);
      await box.press("Enter");
      await expect(keyCell(page, KICK)).toHaveText(shown);
      expect((await calls(page, "set_key")).at(-1).value).toBe(typed.trim());
    });
  }

  for (const typed of ["H", "Am7", "é"]) {
    test(`"${typed}" is refused, Esc gives the old key back`, async ({ page }) => {
      await keyCell(page, BASS).dblclick();
      const box = keyCell(page, BASS).locator("input");
      await box.fill(typed);
      await box.press("Enter");
      await expect(box).toHaveClass(/invalid/);
      await box.press("Escape");
      await expect(keyCell(page, BASS)).toHaveText("Am");
      await page.locator("#error-close").click();
    });
  }
});
