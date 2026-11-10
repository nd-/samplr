// The Loop and Key cells: switching a loop with a double-click and back to
// automatic, a key typed in any spelling, and editing that stays silent.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

// In the fake library: a kick, a one-shot without a key, and a bass loop in
// Am, both from their names; Snare 12 just before the bass.
const KICK = 0;
const SNARE = 23;
const BASS = 24;

const loopCell = (page, index) => row(page, index).locator("td.loop");
const keyCell = (page, index) => row(page, index).locator("td.key");

test("a double-click switches Loop, a second goes back to automatic", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await expect(loopCell(page, KICK)).toHaveText("—");
  await loopCell(page, KICK).dblclick();
  await expect(loopCell(page, KICK)).toHaveText("✓");
  await expect(loopCell(page, KICK)).toHaveAttribute("title", /^Loop, set by hand/);
  expect((await calls(page, "set_loop")).at(-1)).toEqual({
    paths: ["/lib/Drums/Kick 1.wav"],
    looped: true,
  });

  // Back to the value worked out: not kept as set by hand.
  await loopCell(page, KICK).dblclick();
  await expect(loopCell(page, KICK)).toHaveText("—");
  expect((await calls(page, "set_loop")).at(-1)).toEqual({
    paths: ["/lib/Drums/Kick 1.wav"],
    looped: null,
  });
  await expect(loopCell(page, KICK)).not.toHaveAttribute("title", /set by hand/);
});

test("Loop switched for a selection, each sample as it needs", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, SNARE).click();
  await name(page, BASS).click({ modifiers: ["Shift"] });
  // The snare becomes a loop: set by hand. The bass is one already: it goes
  // back to automatic rather than being set by hand.
  await loopCell(page, SNARE).dblclick();
  expect(await calls(page, "set_loop")).toEqual([
    { paths: ["/lib/Loops/Bass 1 120BPM Am.wav"], looped: null },
    { paths: ["/lib/Drums/Snare 12.wav"], looped: true },
  ]);
  await expect(loopCell(page, SNARE)).toHaveText("✓");
  await expect(loopCell(page, BASS)).toHaveText("✓");
});

test("a key typed in any spelling is kept in sharps", async ({ page }) => {
  await keyCell(page, KICK).dblclick();
  const box = keyCell(page, KICK).locator("input");
  await box.fill("bbm");
  await box.press("Enter");
  await expect(keyCell(page, KICK)).toHaveText("A#m");
  expect((await calls(page, "set_key")).at(-1)).toEqual({
    paths: ["/lib/Drums/Kick 1.wav"],
    value: "bbm",
  });
});

test("a key refused keeps the box open, Esc gives the old one back", async ({ page }) => {
  await keyCell(page, BASS).dblclick();
  const box = keyCell(page, BASS).locator("input");
  await box.fill("H");
  await box.press("Enter");
  await expect(box).toHaveClass(/invalid/);
  await expect(page.locator("#error-text")).toContainText("is not a key");
  await box.press("Escape");
  await expect(keyCell(page, BASS)).toHaveText("Am");
  await page.locator("#error-close").click();
});

test("an empty key means none, Automatic key brings the name's back", async ({ page }) => {
  await keyCell(page, BASS).dblclick();
  await keyCell(page, BASS).locator("input").fill("");
  await keyCell(page, BASS).locator("input").press("Enter");
  await expect(keyCell(page, BASS)).toHaveText("");

  await page.locator("#autoplay").uncheck();
  await name(page, BASS).click({ button: "right" });
  await page.locator("#context-menu button", { hasText: "Automatic key" }).click();
  await expect(keyCell(page, BASS)).toHaveText("Am");
  expect((await calls(page, "set_key")).at(-1).value).toBe("auto");
});

test("editing a cell stays silent; a single click on it plays, a little later", async ({
  page,
}) => {
  await keyCell(page, KICK).dblclick();
  await expect(keyCell(page, KICK).locator("input")).toBeVisible();
  await page.waitForTimeout(500);
  expect(await calls(page, "play")).toHaveLength(0);
  await keyCell(page, KICK).locator("input").press("Escape");

  await loopCell(page, KICK + 1).click();
  // Not at once: a second click could still make it an edit.
  expect(await calls(page, "play")).toHaveLength(0);
  await expect.poll(() => calls(page, "play")).toHaveLength(1);
});
