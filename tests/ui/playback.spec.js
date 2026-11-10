// Playback: play and stop, Space, Loop, the playhead, a click on the
// waveform, volume, pitch and tempo. The sound itself is Rust's: here, what
// the page asks for and what it shows.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

// In the fake library: a one-shot of 0.5 s, and a loop of 2 s at 120 BPM.
const KICK = 0;
const BASS = 24;

const playButton = (page) => page.locator("#play");
const lastPlay = async (page) => (await calls(page, "play")).at(-1);
const emit = (page, name, payload) =>
  page.evaluate(([name, payload]) => window.__emit(name, payload), [name, payload]);

test("playing, then Esc stops", async ({ page }) => {
  await name(page, KICK).click();
  await expect(playButton(page)).toHaveText("⏸");
  await expect(page.locator("#status")).toHaveText("▶ Kick 1.wav");
  await expect(page.locator("#stop")).toBeEnabled();

  await page.keyboard.press("Escape");
  await expect(playButton(page)).toHaveText("▶");
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect(page.locator("#stop")).toBeDisabled();
  expect(await calls(page, "stop")).toHaveLength(1);
});

test("the end of a sound already replaced is ignored", async ({ page }) => {
  await name(page, KICK).click();
  const first = (await lastPlay(page)).id;
  await name(page, KICK + 1).click();
  const second = (await lastPlay(page)).id;
  expect(second).not.toBe(first);

  await emit(page, "playback-ended", first);
  await expect(playButton(page)).toHaveText("⏸");
  await emit(page, "playback-ended", second);
  await expect(playButton(page)).toHaveText("▶");
});

test("Space plays a one-shot again from the start, as a pad", async ({ page }) => {
  await name(page, KICK).click();
  await page.keyboard.press(" ");
  expect(await calls(page, "play")).toHaveLength(2);
  expect(await calls(page, "pause")).toHaveLength(0);
});

test("Space pauses and resumes a loop", async ({ page }) => {
  await name(page, BASS).click();
  await page.keyboard.press(" ");
  await expect(playButton(page)).toHaveText("▶");
  await expect(page.locator("#status")).toHaveText("⏸ Bass 1 120BPM Am.wav");
  await page.keyboard.press(" ");
  await expect(playButton(page)).toHaveText("⏸");
  expect(await calls(page, "pause")).toEqual([{ paused: true }, { paused: false }]);
  expect(await calls(page, "play")).toHaveLength(1);
});

test("with nothing playing, Space and Enter play the selection", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, KICK).click();
  expect(await calls(page, "play")).toHaveLength(0);
  await page.keyboard.press(" ");
  await expect(playButton(page)).toHaveText("⏸");
  await page.keyboard.press("Enter");
  expect(await calls(page, "play")).toHaveLength(2);
});

test("L plays the sound again as a loop, and is remembered", async ({ page }) => {
  await name(page, KICK).click();
  expect((await lastPlay(page)).looping).toBe(false);
  await page.keyboard.press("l");
  await expect(page.locator("#loop")).toBeChecked();
  expect((await lastPlay(page)).looping).toBe(true);

  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect(page.locator("#loop")).toBeChecked();
});

test("a click on the waveform plays from that point", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, BASS).click();
  const wave = page.locator("#wave");
  const box = await wave.boundingBox();
  await wave.click({ position: { x: box.width / 4, y: box.height / 2 } });
  // A quarter of a 2 s loop.
  expect((await lastPlay(page)).start).toBeCloseTo(0.5, 1);
});

test("the playhead follows the position the audio thread reports", async ({ page }) => {
  await name(page, BASS).click();
  const playhead = page.locator("#playhead");
  await expect(playhead).toBeVisible();
  await emit(page, "playback-position", { id: (await lastPlay(page)).id, seconds: 1 });
  // Half of the 2 s loop, moving on meanwhile: read once the next frame has
  // drawn it.
  const left = () => playhead.evaluate((line) => parseFloat(line.style.left));
  await expect.poll(left).toBeGreaterThanOrEqual(50);
  expect(await left()).toBeLessThan(60);

  await page.keyboard.press("Escape");
  await expect(playhead).toBeHidden();
});

test("the volume slider acts squared, and is remembered", async ({ page }) => {
  await page.locator("#volume").fill("50");
  expect((await calls(page, "set_volume")).at(-1).volume).toBeCloseTo(0.25);
  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect(page.locator("#volume")).toHaveValue("50");
});

test("pitch and tempo, one without the other or together with Tape", async ({ page }) => {
  await name(page, BASS).click();
  await page.locator("#pitch").fill("12");
  await expect(page.locator("#pitch-value")).toHaveText("+12 st");
  expect((await calls(page, "set_playback")).at(-1)).toEqual({
    speed: 1,
    tempo: 1,
    semitones: 12,
  });

  // The loop's 120 BPM played at 60.
  await page.locator("#target-bpm").fill("60");
  expect((await calls(page, "set_playback")).at(-1)).toEqual({
    speed: 1,
    tempo: 0.5,
    semitones: 12,
  });

  // Tape: one speed, an octave up and half the tempo cancel out.
  await page.locator("#tape").check();
  expect((await calls(page, "set_playback")).at(-1)).toEqual({
    speed: 1,
    tempo: 1,
    semitones: 0,
  });

  // A double-click brings the pitch back to 0.
  await page.locator("#pitch").dblclick();
  await expect(page.locator("#pitch-value")).toHaveText("0 st");
});

test("a sample without a tempo is left at its own", async ({ page }) => {
  await page.locator("#target-bpm").fill("60");
  await name(page, KICK).click();
  expect((await lastPlay(page)).tempo).toBe(1);
  await name(page, BASS).click();
  expect((await lastPlay(page)).tempo).toBe(0.5);
});
