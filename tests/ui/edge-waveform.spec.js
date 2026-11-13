// Edge cases of the waveform and the playhead: answers coming late or
// failing, odd peaks, odd lengths, clicks at the edges, positions of a sound
// already replaced, speed changes while playing, HiDPI and resizing.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

// In the fake library: Kick 1 lasts 0.5 s, Kick 2 0.6 s, Bass 1 is a loop of
// 2 s at 120 BPM.
const KICK = 0;
const KICK2 = 1;
const BASS = 24;
const KICK_PATH = "/lib/Drums/Kick 1.wav";
const KICK2_PATH = "/lib/Drums/Kick 2.wav";

// The page keeps `invoke` from its first script on: the fake is wrapped
// before the page loads, hence the reload. `__peaksFor(path)` replaces the
// waveform's answer (it may throw), `__playFails` makes `play` fail.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__peaksFor = null;
    window.__playFails = null;
    const original = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (command, args = {}) => {
      const answer = await original(command, args);
      if (command === "waveform" && window.__peaksFor) return window.__peaksFor(args.path);
      if (command === "play" && window.__playFails) throw window.__playFails;
      return answer;
    };
  });
  await reload(page);
});

async function reload(page) {
  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect.poll(() => page.evaluate(() => window.__pending)).toBe(0);
}

const emit = (page, event, payload) =>
  page.evaluate(([event, payload]) => window.__emit(event, payload), [event, payload]);
const lastPlay = async (page) => (await calls(page, "play")).at(-1);
const settled = (page) => expect.poll(() => page.evaluate(() => window.__pending)).toBe(0);

// The same peaks for every sample, `[low, high]` pairs.
async function peaksForAll(page, peaks) {
  await page.evaluate((peaks) => {
    window.__peaksFor = () => peaks;
  }, peaks);
}

const flat = (value, count = 1024) => Array.from({ length: count }, () => [-value, value]);

async function selectSilently(page, index) {
  await page.locator("#autoplay").uncheck();
  await name(page, index).click();
}

// What the canvas shows: for each pixel column, the first and last rows
// painted, or `null`.
function painted(page) {
  return page.evaluate(() => {
    const canvas = document.getElementById("wave");
    const { width, height } = canvas;
    const data = canvas.getContext("2d").getImageData(0, 0, width, height).data;
    const columns = [];
    for (let x = 0; x < width; x++) {
      let top = null;
      let bottom = null;
      for (let y = 0; y < height; y++) {
        if (data[(y * width + x) * 4 + 3] > 0) {
          if (top === null) top = y;
          bottom = y;
        }
      }
      columns.push(top === null ? null : [top, bottom]);
    }
    return { width, height, columns };
  });
}

// "full", "line" (silence), "blank", or "mixed".
async function shape(page) {
  const { height, columns } = await painted(page);
  if (columns.every((column) => column === null)) return "blank";
  if (columns.every((column) => column && column[0] === 0 && column[1] === height - 1)) return "full";
  const middle = height / 2;
  if (columns.every((column) => column && column[0] === middle && column[1] === middle)) return "line";
  return "mixed";
}

test.describe("waveform answers", () => {
  test("a late answer for a sample no longer selected is ignored", async ({ page }) => {
    await page.evaluate((kick) => {
      window.__peaksFor = (path) =>
        Array.from({ length: 1024 }, () => (path === kick ? [-1, 1] : [0, 0]));
      window.__delay = (command, args) => (command === "waveform" && args.path === kick ? 300 : 0);
    }, KICK_PATH);
    await selectSilently(page, KICK);
    await name(page, KICK2).click();
    await settled(page);
    expect(await shape(page)).toBe("line");
  });

  test("a late answer after a new search emptied the selection is ignored", async ({ page }) => {
    await peaksForAll(page, flat(1));
    await page.evaluate(() => {
      window.__delay = (command) => (command === "waveform" ? 300 : 0);
    });
    await selectSilently(page, KICK);
    await page.locator("#search").fill("snare");
    await expect(page.locator("#status")).toHaveText("12 sample(s)");
    await settled(page);
    expect(await shape(page)).toBe("blank");
  });

  test("a failing waveform shows the error and leaves no stale drawing", async ({ page }) => {
    await page.evaluate((kick2) => {
      window.__peaksFor = (path) => {
        if (path === kick2) throw `${path}: unsupported format`;
        return Array.from({ length: 1024 }, () => [-1, 1]);
      };
    }, KICK2_PATH);
    await selectSilently(page, KICK);
    await expect.poll(() => shape(page)).toBe("full");
    await name(page, KICK2).click();
    await expect(page.locator("#error-text")).toHaveText(`${KICK2_PATH}: unsupported format`);
    expect(await shape(page)).toBe("blank");
    await page.locator("#error-close").click();
  });

  // Expected: a failure for a sample the user already left is ignored, like
  // its late success; the error box otherwise blames the sample now shown.
  test("a late failure for a sample no longer selected shows no error", async ({ page }) => {
    await page.evaluate((kick) => {
      window.__peaksFor = (path) => {
        if (path === kick) throw `${path}: unsupported format`;
        return Array.from({ length: 1024 }, () => [0, 0]);
      };
      window.__delay = (command, args) => (command === "waveform" && args.path === kick ? 300 : 0);
    }, KICK_PATH);
    await selectSilently(page, KICK);
    await name(page, KICK2).click();
    await settled(page);
    expect(await shape(page)).toBe("line");
    await expect(page.locator("#error-box")).toBeHidden();
  });
});

test.describe("odd peaks", () => {
  test("silence is a one-pixel line in the middle, in the theme's colour", async ({ page }) => {
    await peaksForAll(page, flat(0));
    await selectSilently(page, KICK);
    await expect.poll(() => shape(page)).toBe("line");
    // The System theme, light: --wave is #C47A08.
    const pixel = await page.evaluate(() => {
      const canvas = document.getElementById("wave");
      return Array.from(canvas.getContext("2d").getImageData(0, canvas.height / 2, 1, 1).data);
    });
    expect(pixel).toEqual([0xc4, 0x7a, 0x08, 255]);
  });

  test("peaks at the byte's extremes fill the whole height", async ({ page }) => {
    // -128 is the one byte value below -1 once divided by 127.
    await peaksForAll(page, flat(1).map(() => [-128 / 127, 127 / 127]));
    await selectSilently(page, KICK);
    await expect.poll(() => shape(page)).toBe("full");
  });

  test("peaks beyond ±1 are clipped, not an error", async ({ page }) => {
    await peaksForAll(page, flat(3));
    await selectSilently(page, KICK);
    await expect.poll(() => shape(page)).toBe("full");
  });

  test("fewer peaks than pixels leave no gap, in their order", async ({ page }) => {
    const peaks = Array.from({ length: 10 }, (_, i) => [-(i + 1) / 10, (i + 1) / 10]);
    await peaksForAll(page, peaks);
    await selectSilently(page, KICK);
    await expect.poll(async () => (await painted(page)).columns.every((c) => c !== null)).toBe(true);
    const { columns, height } = await painted(page);
    const heights = columns.map(([top, bottom]) => bottom - top);
    for (let x = 1; x < heights.length; x++) expect(heights[x]).toBeGreaterThanOrEqual(heights[x - 1]);
    expect(heights.at(-1)).toBeGreaterThanOrEqual(height - 2);
    expect(heights[0]).toBeLessThan(height / 5);
  });

  test("a single peak is spread over the whole width", async ({ page }) => {
    await peaksForAll(page, [[-1, 1]]);
    await selectSilently(page, KICK);
    await expect.poll(() => shape(page)).toBe("full");
  });

  test("an empty answer leaves the canvas blank", async ({ page }) => {
    await peaksForAll(page, flat(1));
    await selectSilently(page, KICK);
    await expect.poll(() => shape(page)).toBe("full");
    await peaksForAll(page, []);
    await name(page, KICK2).click();
    await settled(page);
    expect(await shape(page)).toBe("blank");
  });
});

test.describe("odd lengths", () => {
  async function lasting(page, duration, shown) {
    await page.evaluate(
      ([path, duration]) => {
        window.__library.set(path, { duration_secs: duration });
        window.__emit("library-changed", null);
      },
      [KICK_PATH, duration],
    );
    await expect(row(page, KICK).locator("td.number").first()).toHaveText(shown);
    await settled(page);
  }

  async function clickWave(page, x) {
    await page.locator("#wave").click({ position: { x, y: 10 } });
  }

  for (const [duration, shown] of [
    [0, "0.00 s"],
    [-1, "-1.00 s"],
    [null, "?"],
  ]) {
    test(`a click on the waveform of a sample lasting ${duration} is ignored`, async ({ page }) => {
      await lasting(page, duration, shown);
      await selectSilently(page, KICK);
      await clickWave(page, 50);
      expect(await calls(page, "play")).toHaveLength(0);
    });

    test(`a sample lasting ${duration} plays without a playhead`, async ({ page }) => {
      await lasting(page, duration, shown);
      await name(page, KICK).click();
      await expect(page.locator("#play")).toHaveText("⏸");
      expect((await lastPlay(page)).start).toBe(0);
      await expect(page.locator("#playhead")).toBeHidden();
    });
  }

  test("a tiny length gives a finite start within it", async ({ page }) => {
    await lasting(page, 0.001, "0.00 s");
    await selectSilently(page, KICK);
    const box = await page.locator("#wave").boundingBox();
    await clickWave(page, box.width / 2);
    const { start } = await lastPlay(page);
    expect(Number.isFinite(start)).toBe(true);
    expect(start).toBeGreaterThan(0);
    expect(start).toBeLessThanOrEqual(0.001);
    await expect.poll(() => page.locator("#playhead").evaluate((line) => line.style.left)).toMatch(/^[\d.]+%$/);
  });

  test("a click on the waveform with nothing selected does nothing", async ({ page }) => {
    await clickWave(page, 50);
    expect(await calls(page, "play")).toHaveLength(0);
  });

  test("the first pixel plays from the start, the last one just before the end", async ({ page }) => {
    await selectSilently(page, KICK);
    const box = await page.locator("#wave").boundingBox();
    await clickWave(page, 0);
    expect((await lastPlay(page)).start).toBe(0);
    await clickWave(page, box.width - 0.01);
    const { start } = await lastPlay(page);
    expect(start).toBeLessThanOrEqual(0.5);
    expect(start).toBeGreaterThan(0.49);
  });
});

test.describe("playhead", () => {
  // The page's clock is ours: the line moves only when the test lets time
  // pass, so that its place can be computed.
  test.beforeEach(async ({ page }) => {
    await page.clock.install();
    await reload(page);
    await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 1000);
  });

  const left = async (page) =>
    Number.parseFloat(await page.locator("#playhead").evaluate((line) => line.style.left));

  test("a position for a sound already replaced is ignored", async ({ page }) => {
    await name(page, KICK).click();
    const first = (await lastPlay(page)).id;
    await name(page, KICK2).click();
    const second = (await lastPlay(page)).id;

    await emit(page, "playback-position", { id: first, seconds: 0.3 });
    await page.clock.runFor(60);
    // 60 ms of Kick 2's 0.6 s, not the 0.3 s of the old sound.
    expect(await left(page)).toBeLessThan(15);

    await emit(page, "playback-position", { id: second, seconds: 0.3 });
    await page.clock.runFor(20);
    expect(await left(page)).toBeGreaterThan(49);
    expect(await left(page)).toBeLessThan(56);
  });

  test("a position after Stop does not bring the line back", async ({ page }) => {
    await name(page, KICK).click();
    const { id } = await lastPlay(page);
    await page.locator("#stop").click();
    await emit(page, "playback-position", { id, seconds: 0.2 });
    await page.clock.runFor(50);
    await expect(page.locator("#playhead")).toBeHidden();
  });

  test("in loop mode a position beyond the length wraps round", async ({ page }) => {
    await page.locator("#loop").check();
    await name(page, BASS).click();
    const { id, looping } = await lastPlay(page);
    expect(looping).toBe(true);
    await emit(page, "playback-position", { id, seconds: 5 });
    await page.clock.runFor(20);
    expect(await left(page)).toBeGreaterThan(49);
    expect(await left(page)).toBeLessThan(52);
  });

  test("played once, a position beyond the length stops at the end", async ({ page }) => {
    await name(page, KICK).click();
    const { id } = await lastPlay(page);
    await emit(page, "playback-position", { id, seconds: 5 });
    await page.clock.runFor(20);
    expect(await left(page)).toBe(100);
  });

  test("the line follows pitch, BPM and Tape changed while playing", async ({ page }) => {
    // The line is drawn at animation frames, every 16 ms of the page's clock:
    // it may lag that much behind, times the speed.
    const near = async (expected, speed) => {
      const value = await left(page);
      expect(value).toBeLessThanOrEqual(expected + 0.1);
      expect(value).toBeGreaterThanOrEqual(expected - ((0.016 * speed) / 2) * 100 - 0.1);
    };
    await name(page, BASS).click();
    await page.clock.runFor(500);
    await near(25, 1);

    // 240 BPM from 120: twice as fast, stretched.
    await page.locator("#target-bpm").fill("240");
    await page.clock.runFor(250);
    await near(50, 2);

    // Tape at the same tempo: the same speed; an octave up then doubles it.
    await page.locator("#tape").check();
    await page.locator("#pitch").fill("12");
    await page.clock.runFor(125);
    await near(75, 4);

    // Tape off: the octave no longer changes the tempo.
    await page.locator("#tape").uncheck();
    await page.clock.runFor(125);
    await near(87.5, 2);
  });

  // Expected: a sound that could not start leaves the bar as stopped (▶,
  // Stop greyed), not showing a sound playing.
  test("a play that fails leaves the bar stopped", async ({ page }) => {
    await page.evaluate(() => {
      window.__playFails = "cannot open the audio output";
    });
    await name(page, KICK).click();
    await expect(page.locator("#error-text")).toHaveText("cannot open the audio output");
    await expect(page.locator("#playhead")).toBeHidden();
    await expect(page.locator("#play")).toHaveText("▶");
    await expect(page.locator("#stop")).toBeDisabled();
    await page.locator("#error-close").click();
  });
});

test.describe("canvas size", () => {
  test("resizing the window draws again at the new width", async ({ page }) => {
    await peaksForAll(page, flat(1));
    await selectSilently(page, KICK);
    await expect.poll(() => shape(page)).toBe("full");
    const before = await painted(page);
    await page.setViewportSize({ width: 1000, height: 700 });
    await expect
      .poll(() => page.locator("#wave").evaluate((c) => c.width === c.clientWidth && c.width !== 0))
      .toBe(true);
    const after = await painted(page);
    expect(after.width).toBeLessThan(before.width);
    expect(await shape(page)).toBe("full");
  });

  test.describe("on a HiDPI screen", () => {
    test.use({ deviceScaleFactor: 2 });

    test("the canvas has twice the pixels, and silence is still one line", async ({ page }) => {
      await peaksForAll(page, flat(0));
      await selectSilently(page, KICK);
      await expect.poll(() => shape(page)).toBe("line");
      const size = await page
        .locator("#wave")
        .evaluate((c) => [c.width, c.height, c.clientWidth, c.clientHeight]);
      expect(size[0]).toBe(size[2] * 2);
      expect(size[1]).toBe(size[3] * 2);
    });

    test("a click maps CSS pixels, not device pixels", async ({ page }) => {
      await selectSilently(page, KICK);
      const box = await page.locator("#wave").boundingBox();
      await page.locator("#wave").click({ position: { x: box.width * 0.75, y: 10 } });
      expect((await lastPlay(page)).start).toBeCloseTo(0.375, 2);
    });
  });
});
