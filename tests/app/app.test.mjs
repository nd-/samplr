// The real app, in WebKitGTK, with its Rust side: what the interface's tests
// in Chromium cannot see. Each test starts an app of its own (see
// webdriver.mjs). Run with `node --test tests/app` on Linux, where
// tauri-driver and WebKitWebDriver are installed, with a display.

import assert from "node:assert/strict";
import { test } from "node:test";
import { startApp } from "./webdriver.mjs";

const ENTER = "";
const row = (index) => `#samples tr[data-index="${index}"]`;
const statusText = 'return document.getElementById("status").textContent';

// Each test gets a fresh app, stopped and deleted afterwards.
function withApp(name, body) {
  test(name, async () => {
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
