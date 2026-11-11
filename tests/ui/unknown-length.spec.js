// A sample whose length is unknown (some MP3s): "?" in the list, left out
// of the playback bar and the Info panel, no playhead, and a click on its
// waveform ignored.
const { test, expect, calls, openFakeLibrary, row, name } = require("./helpers");

openFakeLibrary();

const KICK = 0;

test.beforeEach(async ({ page }) => {
  await page.evaluate(() => {
    window.__library.set("/lib/Drums/Kick 1.wav", { duration_secs: null });
    window.__emit("library-changed", null);
  });
  await expect(row(page, KICK).locator("td.number").first()).toHaveText("?");
});

test("it plays, without a playhead", async ({ page }) => {
  await name(page, KICK).click();
  await expect(page.locator("#play")).toHaveText("⏸");
  await expect(page.locator("#playhead")).toBeHidden();
  await expect(page.locator("#now-details")).toHaveText("One-shot · -12.0 LUFS");
});

test("a click on its waveform is ignored", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, KICK).click();
  await page.locator("#wave").click({ position: { x: 50, y: 10 } });
  expect(await calls(page, "play")).toHaveLength(0);
});

test("its Info panel shows no length", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, KICK).click();
  await expect(page.locator('#info-list dt:text-is("Name")')).toBeVisible();
  await expect(page.locator('#info-list dt:text-is("Length")')).toHaveCount(0);
});
