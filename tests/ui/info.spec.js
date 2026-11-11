// The Info panel: the current sample's details and where they come from,
// what the file says of itself, and folding the panel.
const { test, expect, openFakeLibrary, calls, name } = require("./helpers");

openFakeLibrary();

// In the fake library: Kick 1, whose file carries metadata, and a bass loop
// of 2 s at 120 BPM in Am, mono.
const KICK = 0;
const BASS = 24;

const panel = (page) => page.locator("#info");
const list = (page) => page.locator("#info-list");

// The value shown for `label`, its origin included.
const value = (page, label) =>
  list(page).locator(`dt:text-is("${label}") + dd`);

async function select(page, index) {
  await page.locator("#autoplay").uncheck();
  await name(page, index).click();
}

test("without a selection, the panel says what to do", async ({ page }) => {
  await expect(list(page)).toHaveText("Select a sample to see its details.");
});

test("a sample's details and where they come from", async ({ page }) => {
  await select(page, BASS);
  await expect(value(page, "Name")).toHaveText("Bass 1 120BPM Am.wav");
  await expect(value(page, "Folder")).toHaveText("/lib/Loops");
  await expect(value(page, "Format")).toHaveText("WAV · pcm_s24le · 24 bit · 44.1 kHz · mono");
  // Two seconds at 120 BPM: one bar of four beats.
  await expect(value(page, "Length")).toHaveText("2.00 s · 1 bar");
  await expect(value(page, "Size")).toHaveText("2.4 MB");
  await expect(value(page, "Tempo")).toHaveText("120 BPM from the name");
  await expect(value(page, "Key")).toHaveText("Am");
  await expect(value(page, "Loop")).toHaveText("Loop from the words of its name or folders");
  // Nothing the file says of itself: no such heading.
  await expect(list(page).locator("h3")).toHaveCount(0);
});

test("what the file says of itself, under its own heading", async ({ page }) => {
  await select(page, KICK);
  await expect(list(page).locator("h3")).toHaveText("In the file");
  await expect(value(page, "Artist")).toHaveText("Hutch");
  await expect(value(page, "Genre")).toHaveText("Dub");
  // A one-shot: no bars after its length.
  await expect(value(page, "Length")).not.toContainText("bar");
});

test("a file that cannot be read says why, the rest still shown", async ({ page }) => {
  await page.evaluate(() => window.__unreadable.push("/lib/Drums/Kick 2.wav"));
  await select(page, KICK + 1);
  await expect(value(page, "Error")).toHaveText("/lib/Drums/Kick 2.wav: permission denied");
  await expect(value(page, "Name")).toHaveText("Kick 2.wav");
  await expect(value(page, "Size")).toHaveCount(0);
});

test("a multiple selection is counted", async ({ page }) => {
  await select(page, KICK);
  await name(page, KICK + 2).click({ modifiers: ["Shift"] });
  await expect(value(page, "Selection")).toHaveText("3 samples");
});

test("Ctrl+I folds the panel, which then reads no file, and is remembered", async ({ page }) => {
  await select(page, KICK);
  await expect(value(page, "Artist")).toHaveText("Hutch");
  await page.keyboard.press("ControlOrMeta+i");
  await expect(panel(page)).toHaveClass(/collapsed/);
  await expect(list(page)).toBeHidden();

  // Folded, another sample's file is not read.
  const read = (await calls(page, "file_info")).length;
  await name(page, BASS).click();
  await page.waitForTimeout(100);
  expect(await calls(page, "file_info")).toHaveLength(read);

  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect(panel(page)).toHaveClass(/collapsed/);

  // Unfolded with its button: the current sample's file is read then.
  await page.locator("#autoplay").uncheck();
  await name(page, KICK).click();
  await page.locator("#toggle-info").click();
  await expect(panel(page)).not.toHaveClass(/collapsed/);
  await expect(value(page, "Artist")).toHaveText("Hutch");
});
