// The row menu's actions on the files themselves: rename, move, trash,
// convert, show in the file manager, and Detect BPM.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

const menu = (page) => page.locator("#context-menu");
const entry = (page, text) => menu(page).locator("button", { hasText: text });
const asked = (page) => page.evaluate(() => window.__asked);
const set = (page, values) => page.evaluate((values) => Object.assign(window, values), values);

// In the fake library: a mono kick, a stereo one, and a loop named for its
// tempo.
const MONO = 0;
const STEREO = 1;
const BASS = 24;

// Selects rows `first` to `last` without playing, then opens their menu.
async function menuOf(page, first, last = first) {
  await page.locator("#autoplay").uncheck();
  await name(page, first).click();
  if (last !== first) await name(page, last).click({ modifiers: ["Shift"] });
  await name(page, first).click({ button: "right" });
  await expect(menu(page)).toBeVisible();
}

test("a file renamed keeps its extension, its selection and its sound", async ({ page }) => {
  await name(page, MONO).click();
  await name(page, MONO).click({ button: "right" });
  await entry(page, "Rename file…").click();
  const box = menu(page).locator("input[title]");
  await expect(box).toBeFocused();
  await expect(box).toHaveValue("Kick 1");
  await expect(box).toHaveAttribute("title", "The extension .wav is kept");

  await box.fill("Kick dry");
  await box.press("Enter");
  await expect(menu(page)).toBeHidden();
  await expect(name(page, MONO)).toHaveText("Kick dry.wav");
  await expect(row(page, MONO)).toHaveClass(/selected/);
  expect((await calls(page, "rename_sample")).at(-1)).toEqual({
    path: "/lib/Drums/Kick 1.wav",
    name: "Kick dry",
  });
  // The sound playing followed the new path: nothing stopped.
  await expect(page.locator("#play")).toHaveText("⏸");
  expect(await calls(page, "stop")).toHaveLength(0);
});

test("a name refused keeps the box open, selected", async ({ page }) => {
  await menuOf(page, MONO);
  await entry(page, "Rename file…").click();
  const box = menu(page).locator("input[title]");
  await box.fill("../Kick");
  await box.press("Enter");
  await expect(page.locator("#error-text")).toContainText("is not a file name");
  await expect(box).toBeVisible();
  await expect(name(page, MONO)).toHaveText("Kick 1.wav");
  await box.press("Escape");
  await expect(menu(page)).toBeHidden();
  await page.locator("#error-close").click();
});

test("renaming is offered for a single sample only", async ({ page }) => {
  await menuOf(page, MONO, STEREO);
  await expect(entry(page, "Rename file…")).toHaveCount(0);
  await expect(entry(page, "Move file(s) to folder…")).toBeVisible();
});

test("files moved to a folder of the library, the selection follows", async ({ page }) => {
  await menuOf(page, MONO, STEREO);
  await set(page, { __chosenFolder: "/lib/Pads" });
  await entry(page, "Move file(s) to folder…").click();
  await expect(page.locator("#status")).toHaveText("2 file(s) moved to /lib/Pads");
  expect((await calls(page, "move_samples")).at(-1)).toEqual({
    paths: ["/lib/Drums/Kick 1.wav", "/lib/Drums/Kick 2.wav"],
    folder: "/lib/Pads",
  });
  // Still selected, wherever they are now in the list.
  await expect(page.locator("#samples tr.selected td.folder")).toHaveText(["/lib/Pads", "/lib/Pads"]);
});

test("a folder outside the library is refused, nothing moves", async ({ page }) => {
  await menuOf(page, MONO);
  await set(page, { __chosenFolder: "/elsewhere" });
  await entry(page, "Move file(s) to folder…").click();
  await expect(page.locator("#error-text")).toContainText("is not in a scanned folder");
  await expect(row(page, MONO).locator("td.folder")).toHaveText("/lib/Drums");
  await page.locator("#error-close").click();
});

test("the trash is always asked about, even with confirmations off", async ({ page }) => {
  await page.evaluate(() => localStorage.setItem("askBeforeDeleting", "false"));
  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");

  await menuOf(page, MONO);
  await set(page, { __answer: false });
  await entry(page, "Move file(s) to the trash…").click();
  await expect.poll(() => asked(page)).toHaveLength(1);
  expect((await asked(page))[0]).toContain(
    "Their tags, collections and favorite are lost, even if put back from the trash.",
  );
  expect(await calls(page, "trash_samples")).toHaveLength(0);

  await menuOf(page, MONO);
  await set(page, { __answer: true });
  await entry(page, "Move file(s) to the trash…").click();
  await expect(page.locator("#status")).toHaveText("1 file(s) moved to the trash");
  await expect(name(page, MONO)).toHaveText("Kick 2.wav");
});

test("Mix down to mono is offered for a stereo sample only", async ({ page }) => {
  const mono = page.locator("#convert-mono-box");
  await menuOf(page, MONO);
  await entry(page, "Convert file(s)…").click();
  await expect(page.locator("#convert-title")).toHaveText("Convert Kick 1.wav");
  await expect(mono).toBeHidden();
  await page.keyboard.press("Escape");

  await menuOf(page, MONO, STEREO);
  await entry(page, "Convert file(s)…").click();
  await expect(page.locator("#convert-title")).toHaveText("Convert 2 files");
  await expect(mono).toBeVisible();
});

test("a conversion writes copies, and its settings are remembered", async ({ page }) => {
  await menuOf(page, STEREO);
  await entry(page, "Convert file(s)…").click();
  await page.locator("#convert-format").selectOption("mp3");
  // Only the chosen format's settings show.
  await expect(page.locator("#convert-mp3")).toBeVisible();
  await expect(page.locator("#convert-bits")).toBeHidden();
  await page.locator("#convert-mp3").selectOption("192");
  await page.locator("#convert-mono").check();
  await page.locator("#convert button", { hasText: "Convert" }).last().click();

  await expect(page.locator("#status")).toHaveText("1 copy(ies) written, next to the original(s)");
  expect((await calls(page, "convert_samples")).at(-1)).toEqual({
    paths: ["/lib/Drums/Kick 2.wav"],
    options: { format: "mp3", bits: null, mp3Bitrate: 192, oggQuality: 8, mono: true },
  });
  // The copy, at the end of the list, found by a search.
  await page.locator("#search").fill("mono.mp3");
  await expect(page.locator("#status")).toHaveText("1 sample(s)");
  await expect(name(page, 0)).toHaveText("Kick 2 mono.mp3");
  await page.locator("#search").fill("");

  // Cancelled, nothing is converted; the last choices come back.
  await menuOf(page, STEREO);
  await entry(page, "Convert file(s)…").click();
  await expect(page.locator("#convert-format")).toHaveValue("mp3");
  await expect(page.locator("#convert-mp3")).toHaveValue("192");
  await page.locator("#convert button", { hasText: "Cancel" }).click();
  await expect(page.locator("#convert")).toBeHidden();
  expect(await calls(page, "convert_samples")).toHaveLength(1);
});

test("converting is greyed when ffmpeg is not found", async ({ page }) => {
  await page.addInitScript(() => {
    window.__ffmpeg = false;
  });
  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await menuOf(page, MONO);
  await expect(entry(page, "Convert file(s)…")).toBeDisabled();
  await expect(entry(page, "Convert file(s)…")).toHaveAttribute(
    "title",
    "Converting needs ffmpeg, not found on this computer",
  );
});

test("Show in file manager asks for the selection", async ({ page }) => {
  await menuOf(page, MONO, STEREO);
  await entry(page, "Show in file manager").click();
  expect((await calls(page, "reveal")).at(-1).paths).toHaveLength(2);
});

test("Detect BPM shows what each method gives, greyed when nothing", async ({ page }) => {
  await menuOf(page, BASS);
  await entry(page, "Detect BPM…").click();
  await expect(entry(page, "From the name")).toHaveText("From the name: 120");
  await expect(entry(page, "From the file")).toBeDisabled();
  await expect(entry(page, "From the sound")).toHaveText("From the sound");

  await entry(page, "From the name").click();
  await expect(page.locator("#status")).toHaveText("Tempo detected for 1 sample(s)");
  expect((await calls(page, "detect_tempo")).at(-1)).toEqual({
    paths: ["/lib/Loops/Bass 1 120BPM Am.wav"],
    method: "name",
  });
});

test("Detect BPM on a selection says how many gave no tempo", async ({ page }) => {
  await menuOf(page, MONO, STEREO);
  await entry(page, "Detect BPM…").click();
  // Several samples: no value shown, the method applies to each.
  await expect(entry(page, "From the name")).toHaveText("From the name");
  await entry(page, "From the name").click();
  await expect(page.locator("#error-text")).toHaveText(
    "No tempo found for 2 sample(s): no tempo in their name",
  );
  await page.locator("#error-close").click();
});

test("Automatic in Detect BPM brings back the tempo worked out", async ({ page }) => {
  await menuOf(page, BASS);
  await entry(page, "Detect BPM…").click();
  await entry(page, "Automatic").first().click();
  expect((await calls(page, "set_tempo")).at(-1)).toEqual({
    paths: ["/lib/Loops/Bass 1 120BPM Am.wav"],
    value: "auto",
  });
});
