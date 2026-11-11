// The real app, in WebKitGTK, with its Rust side: what the interface's tests
// in Chromium cannot see. Each test starts an app of its own (see
// webdriver.mjs). Run with `node --test tests/app` on Linux, where
// tauri-driver and WebKitWebDriver are installed, with a display.

import assert from "node:assert/strict";
import { test } from "node:test";
import { spawnSync } from "node:child_process";
import { existsSync, rmSync, renameSync } from "node:fs";
import { join } from "node:path";
import { startApp, writeWav } from "./webdriver.mjs";

const ENTER = "";
const row = (index) => `#samples tr[data-index="${index}"]`;
const statusText = 'return document.getElementById("status").textContent';

// Each test gets a fresh app, stopped and deleted afterwards. `options`:
// node:test's (`skip`).
function withApp(name, body, options = {}) {
  test(name, options, async () => {
    const app = await startApp();
    try {
      await body(app);
    } finally {
      await app.close();
    }
  });
}

withApp("the page loads every script and lists the library", async (app) => {
  const scripts = await app.run(
    "return [...document.scripts].map((script) => script.getAttribute('src'))",
  );
  assert.deepEqual(scripts, [
    "theme.js",
    "base.js",
    "list.js",
    "details.js",
    "player.js",
    "library.js",
    "collections.js",
    "menus.js",
    "keys.js",
    "cells.js",
    "settings.js",
    "main.js",
  ]);
  // The last file's functions are there: every file ran.
  assert.equal(await app.run("return typeof renderSettings"), "function");
  const names = await app.run(
    "return [...document.querySelectorAll('#samples tr[data-index] td:nth-child(2)')].map((td) => td.textContent)",
  );
  assert.deepEqual(names, ["Bass Loop 120BPM Am.wav", "Kick 01.wav", "Snare 02.wav"]);
  // Worked out by Rust from the name: tempo, key, loop.
  assert.equal(await app.run(`return document.querySelector('${row(0)} td.bpm').textContent`), "120");
  assert.equal(await app.run(`return document.querySelector('${row(0)} td.key').textContent`), "Am");
});

withApp("a search goes through Rust's search language", async (app) => {
  await app.type("#search", "bpm:118-124");
  await app.waitFor(statusText, "1 sample(s)");
  await app.run('const box = document.getElementById("search"); box.value = ""; box.dispatchEvent(new Event("input"));');
  await app.type("#search", "snare");
  // The status already said 1 sample: the row tells the new answer came.
  await app.waitFor(
    `return document.querySelector('${row(0)} td:nth-child(2)').textContent`,
    "Snare 02.wav",
  );
  assert.equal(await app.run(statusText), "1 sample(s)");
});

withApp("tags typed in the page reach the database", async (app) => {
  await app.click("#autoplay");
  await app.click(`${row(1)} td:nth-child(2)`);
  await app.type("#new-tags", `Punchy dry${ENTER}`);
  await app.waitFor(
    'return [...document.querySelectorAll("#tagbar .tag")].map((chip) => chip.firstChild.textContent)',
    ["dry", "punchy"],
  );
  // The command line, on the same database, sees them.
  assert.deepEqual(app.cli("tags"), [
    { name: "dry", count: 1 },
    { name: "punchy", count: 1 },
  ]);
});

withApp("the selected sample's waveform is computed by Rust", async (app) => {
  await app.click("#autoplay");
  await app.click(`${row(0)} td:nth-child(2)`);
  // Two seconds of sound: brought down to 1024 points.
  await app.waitFor("return peaks.length", 1024);
  const loudest = await app.run("return Math.max(...peaks.map(([low, high]) => Math.max(-low, high)))");
  assert.ok(loudest > 0.2 && loudest < 0.3, `peak ${loudest}`);
});

withApp("a sample played reaches its end through the audio thread", async (app) => {
  // Silent: the test plays nothing aloud.
  await app.run('const v = document.getElementById("volume"); v.value = 0; v.dispatchEvent(new Event("input"));');
  await app.click(`${row(1)} td:nth-child(2)`);
  await app.waitFor('return document.getElementById("play").textContent', "⏸");
  // Half a second of sound: the end comes back from Rust.
  await app.waitFor('return document.getElementById("play").textContent', "▶", 5_000);
  assert.equal(await app.run(statusText), "3 sample(s)");
});

withApp("the rows' columns line up with the header in WebKitGTK", async (app) => {
  // The header is a table of its own: each of its cells must sit right
  // above the cells of the rows (a `<col>` WebKit sized otherwise would
  // shift them).
  const offsets = await app.run(`
    const heads = [...document.querySelectorAll("#list-head th")].filter((th) => !th.hidden);
    const cells = [...document.querySelector('${row(0)}').children];
    return heads.map((th, i) => {
      const a = th.getBoundingClientRect();
      const b = cells[i].getBoundingClientRect();
      return [th.dataset.sort ?? "", Math.round(a.left - b.left), Math.round(a.width - b.width)];
    });`);
  assert.ok(offsets.length > 10, `${offsets.length} columns`);
  for (const [column, left, width] of offsets) {
    assert.ok(Math.abs(left) <= 1 && Math.abs(width) <= 1, `${column}: ${left}, ${width}`);
  }
});

// The names in the list, in its order.
const names = "return [...document.querySelectorAll('#samples tr[data-index] td:nth-child(2)')].map((td) => td.textContent)";
const selectedNames =
  "return [...document.querySelectorAll('#samples tr.selected td:nth-child(2)')].map((td) => td.textContent)";

withApp("a sample added, renamed or deleted on disk follows in the list", async (app) => {
  await app.click("#autoplay");
  await app.click(`${row(1)} td:nth-child(2)`);
  // Added while the app is open: in the list after the watcher's second of
  // grouping, the selection kept.
  writeWav(join(app.samples, "Clap 03.wav"), 0.5);
  await app.waitFor(names, ["Bass Loop 120BPM Am.wav", "Clap 03.wav", "Kick 01.wav", "Snare 02.wav"]);
  assert.deepEqual(await app.run(selectedNames), ["Kick 01.wav"]);

  // Renamed: the same sample, its tags kept.
  await app.click(`${row(1)} td:nth-child(2)`);
  await app.type("#new-tags", `clap${ENTER}`);
  await app.waitFor('return document.querySelectorAll("#tagbar .tag").length', 1);
  renameSync(join(app.samples, "Clap 03.wav"), join(app.samples, "Clap 04.wav"));
  await app.waitFor(names, ["Bass Loop 120BPM Am.wav", "Clap 04.wav", "Kick 01.wav", "Snare 02.wav"]);
  assert.deepEqual(app.cli("search", "#clap").map((sample) => sample.path.split("/").at(-1)), [
    "Clap 04.wav",
  ]);

  // Deleted: gone from the list.
  rmSync(join(app.samples, "Clap 04.wav"));
  await app.waitFor(names, ["Bass Loop 120BPM Am.wav", "Kick 01.wav", "Snare 02.wav"]);
});

withApp("a file renamed from the row menu, on disk and in the database", async (app) => {
  await app.click("#autoplay");
  await app.click(`${row(1)} td:nth-child(2)`);
  await app.type("#new-tags", `kick${ENTER}`);
  await app.waitFor('return document.querySelectorAll("#tagbar .tag").length', 1);

  await app.menu(`${row(1)} td:nth-child(2)`, "Rename file");
  // The name without its extension, which is kept.
  assert.equal(await app.run('return document.querySelector("#context-menu input[title]").value'), "Kick 01");
  await app.clear("#context-menu input[title]");
  await app.type("#context-menu input[title]", `Kick dry${ENTER}`);
  await app.waitFor(names, ["Bass Loop 120BPM Am.wav", "Kick dry.wav", "Snare 02.wav"]);

  assert.ok(existsSync(join(app.samples, "Kick dry.wav")));
  assert.ok(!existsSync(join(app.samples, "Kick 01.wav")));
  // The same sample, its tag kept, and still selected.
  assert.deepEqual(app.cli("search", "#kick").map((sample) => sample.path.split("/").at(-1)), [
    "Kick dry.wav",
  ]);
  assert.deepEqual(await app.run(selectedNames), ["Kick dry.wav"]);
});

const ffmpegFound = spawnSync("ffmpeg", ["-version"]).status === 0;

withApp(
  "a sample converted by ffmpeg, the copy next to it with its tags",
  async (app) => {
    await app.click("#autoplay");
    await app.click(`${row(1)} td:nth-child(2)`);
    await app.type("#new-tags", `kick${ENTER}`);
    await app.waitFor('return document.querySelectorAll("#tagbar .tag").length', 1);

    await app.menu(`${row(1)} td:nth-child(2)`, "Convert file(s)");
    await app.waitFor('return document.getElementById("convert").open', true);
    await app.run(
      'const format = document.getElementById("convert-format"); format.value = "flac"; format.dispatchEvent(new Event("change"));',
    );
    await app.click("#convert-go");
    await app.waitFor(names, ["Bass Loop 120BPM Am.wav", "Kick 01.flac", "Kick 01.wav", "Snare 02.wav"]);

    assert.ok(existsSync(join(app.samples, "Kick 01.flac")));
    assert.ok(existsSync(join(app.samples, "Kick 01.wav")), "the original is kept");
    const copy = app.cli("search", "#kick").find((sample) => sample.path.endsWith(".flac"));
    assert.equal(copy.codec, "flac");
  },
  { skip: !ffmpegFound && "ffmpeg is not installed" },
);

// The colour most drawn on the waveform's canvas, as `#rrggbb`.
const drawnColour = `
  const canvas = document.getElementById("wave");
  const pixels = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
  const counts = new Map();
  for (let i = 0; i < pixels.length; i += 4) {
    if (pixels[i + 3] < 255) continue;
    const hex = "#" + [0, 1, 2].map((c) => pixels[i + c].toString(16).padStart(2, "0")).join("");
    counts.set(hex, (counts.get(hex) ?? 0) + 1);
  }
  return [...counts].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;`;

withApp("the waveform is drawn in the theme's colour", async (app) => {
  await app.click("#autoplay");
  await app.click(`${row(0)} td:nth-child(2)`);
  await app.waitFor("return peaks.length", 1024);
  // The System theme writes its colour as light-dark(), which a canvas
  // does not read: once drawn black. Soul's, light or dark (style.css).
  const system = await app.run(drawnColour);
  assert.ok(["#c47a08", "#f2a93b"].includes(system), `System theme: ${system}`);

  await app.run(
    'const theme = document.getElementById("theme"); theme.value = "dub-dark"; theme.dispatchEvent(new Event("change"));',
  );
  assert.equal(await app.run(drawnColour), "#86c46a");
});

withApp("the app's choices are found again at the next launch", async (app) => {
  await app.run(
    'const theme = document.getElementById("theme"); theme.value = "dub-dark"; theme.dispatchEvent(new Event("change"));',
  );
  await app.run('const v = document.getElementById("volume"); v.value = 30; v.dispatchEvent(new Event("input"));');
  await app.click("#loop");
  // The Codec column hidden from the header's menu.
  await app.run(
    `document.querySelector("#list-head th").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 300, clientY: 100 }));
     [...document.querySelectorAll("#context-menu button")].find((b) => b.textContent.includes("Codec")).click();`,
  );
  await app.waitFor('return document.querySelector(\'th[data-sort="codec"]\').hidden', true);

  await app.restart();
  assert.equal(await app.run("return document.documentElement.dataset.theme"), "dub-dark");
  assert.equal(await app.run('return document.getElementById("volume").value'), "30");
  assert.equal(await app.run('return document.getElementById("loop").checked'), true);
  assert.equal(await app.run('return document.querySelector(\'th[data-sort="codec"]\').hidden'), true);
  // Not the window's size: tauri-plugin-window-state saves it when Tauri
  // closes the window, which WebDriver cannot ask for (its own close goes
  // round Tauri, and the page may not close the window).
});
