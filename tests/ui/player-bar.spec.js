// The playback bar's line about the selected sample: tempo, key, loop,
// length and loudness, as heard once pitched or played at another tempo.
const { test, expect, calls, openFakeLibrary, name } = require("./helpers");

openFakeLibrary();

// In the fake library: a kick of 0.5 s at -12 LUFS, and a bass loop of 2 s
// at 120 BPM in Am, at -16 LUFS.
const KICK = 0;
const BASS = 24;

const details = (page) => page.locator("#now-details");

async function select(page, index) {
  await page.locator("#autoplay").uncheck();
  await name(page, index).click();
}

test("a loop's tempo, key, bars and loudness", async ({ page }) => {
  await select(page, BASS);
  await expect(page.locator("#now-name")).toHaveText("Bass 1 120BPM Am.wav");
  await expect(details(page)).toHaveText("120 BPM · Am · Loop · 2.00 s (1 bar) · -16.0 LUFS");
});

test("a one-shot without tempo nor key", async ({ page }) => {
  await select(page, KICK);
  await expect(details(page)).toHaveText("One-shot · 0.50 s · -12.0 LUFS");
});

test("pitched, the key heard, the file's own between brackets", async ({ page }) => {
  await select(page, BASS);
  await page.locator("#pitch").fill("2");
  await expect(details(page)).toContainText("120 BPM · Bm (Am) · ");
  await page.locator("#pitch").fill("-12");
  await expect(details(page)).toContainText("120 BPM · Am · ");
});

test("played at another tempo, the tempo heard", async ({ page }) => {
  await select(page, BASS);
  await page.locator("#target-bpm").fill("100");
  await expect(details(page)).toContainText("100 BPM (120) · Am · ");
  // With Tape, slower is lower too: 100 from 120 is about 3 semitones down.
  await page.locator("#tape").check();
  await expect(details(page)).toContainText("100 BPM (120) · F#m (Am) · ");
});

test("the Stop button stops, and is greyed when nothing plays", async ({ page }) => {
  const stop = page.locator("#stop");
  await expect(stop).toBeDisabled();
  await name(page, KICK).click();
  await expect(stop).toBeEnabled();
  await stop.click();
  await expect(stop).toBeDisabled();
  expect(await calls(page, "stop")).toHaveLength(1);
});
