// The real app driven through tauri-driver (WebDriver over HTTP), each test
// on an app of its own: a throwaway library and database, and the app's
// settings and web storage in a temporary folder, so that the user's own
// are never touched.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const target = process.env.CARGO_TARGET_DIR ?? join(root, "target");
export const appBinary = join(target, "debug", "samplr-app");
const cliBinary = join(target, "debug", "samplr");
const port = 4444;
const base = `http://127.0.0.1:${port}`;

/// Writes a 16-bit mono WAV of `seconds` of a quiet tone at `path`.
export function writeWav(path, seconds) {
  const frames = Math.round(44100 * seconds);
  const data = Buffer.alloc(frames * 2);
  for (let i = 0; i < frames; i++) data.writeInt16LE(Math.round(8000 * Math.sin(i * 0.05)), i * 2);
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + data.length, 4);
  header.write("WAVEfmt ", 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20); // PCM
  header.writeUInt16LE(1, 22); // mono
  header.writeUInt32LE(44100, 24);
  header.writeUInt32LE(88200, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36);
  header.writeUInt32LE(data.length, 40);
  writeFileSync(path, Buffer.concat([header, data]));
}

const pause = (ms) => new Promise((done) => setTimeout(done, ms));

/// An app of its own: three samples scanned into a throwaway database, the
/// app started through tauri-driver. `close()` stops everything and deletes
/// the folder.
export async function startApp() {
  const dir = mkdtempSync(join(tmpdir(), "samplr-app-"));
  const samples = join(dir, "samples");
  mkdirSync(samples);
  writeWav(join(samples, "Kick 01.wav"), 0.5);
  writeWav(join(samples, "Snare 02.wav"), 0.5);
  writeWav(join(samples, "Bass Loop 120BPM Am.wav"), 2);
  const env = {
    ...process.env,
    SAMPLR_DB: join(dir, "samplr.db"),
    XDG_CONFIG_HOME: join(dir, "config"),
    XDG_DATA_HOME: join(dir, "data"),
  };
  execFileSync(cliBinary, ["scan", samples], { env, stdio: "ignore" });

  const driver = spawn("tauri-driver", ["--port", String(port)], { env, stdio: "ignore" });
  let session = null;
  const app = {
    dir,
    samples,
    env,
    /// The `samplr` command line on the same database, as JSON.
    cli: (...args) => JSON.parse(execFileSync(cliBinary, ["--json", ...args], { env }).toString()),
    async call(method, path, body) {
      const response = await fetch(base + path, {
        method,
        headers: { "content-type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const json = await response.json();
      if (!response.ok) throw new Error(`${method} ${path}: ${JSON.stringify(json.value)}`);
      return json.value;
    },
    /// Runs `script` in the page (a function body) and returns its value.
    run(script, ...args) {
      return app.call("POST", `/session/${session}/execute/sync`, { script, args });
    },
    async element(css) {
      const found = await app.call("POST", `/session/${session}/element`, {
        using: "css selector",
        value: css,
      });
      return Object.values(found)[0];
    },
    async click(css) {
      await app.call("POST", `/session/${session}/element/${await app.element(css)}/click`, {});
    },
    async clear(css) {
      await app.call("POST", `/session/${session}/element/${await app.element(css)}/clear`, {});
    },
    /// Opens the row menu of `css` as a right-click does, and clicks its
    /// entry starting with `label`.
    async menu(css, label) {
      await app.run(
        `const cell = document.querySelector(arguments[0]);
         const box = cell.getBoundingClientRect();
         cell.dispatchEvent(new MouseEvent("contextmenu", {
           bubbles: true, cancelable: true, button: 2,
           clientX: box.left + 10, clientY: box.top + 5,
         }));`,
        css,
      );
      await app.waitFor(
        `return [...document.querySelectorAll("#context-menu button")].some((b) => b.textContent.startsWith(arguments[0]))`,
        true,
        10_000,
        label,
      );
      await app.run(
        `[...document.querySelectorAll("#context-menu button")].find((b) => b.textContent.startsWith(arguments[0])).click()`,
        label,
      );
    },
    /// Types `text` into the element; WebDriver's own codes for special keys
    /// ("" is Enter).
    async type(css, text) {
      await app.call("POST", `/session/${session}/element/${await app.element(css)}/value`, { text });
    },
    /// Waits for `script` to give `expected`; fails with the last value.
    async waitFor(script, expected, timeout = 10_000, ...args) {
      let value;
      for (const start = Date.now(); Date.now() - start < timeout; await pause(100)) {
        value = await app.run(script, ...args);
        if (JSON.stringify(value) === JSON.stringify(expected)) return value;
      }
      throw new Error(`waited for ${JSON.stringify(expected)}, got ${JSON.stringify(value)}`);
    },
    async close() {
      try {
        if (session) await app.call("DELETE", `/session/${session}`);
      } finally {
        driver.kill();
        // The app, still closing, writes its web storage again after a
        // first removal: removed until the folder stays gone.
        for (let tries = 0; tries < 20; tries++) {
          rmSync(dir, { recursive: true, force: true });
          await pause(150);
          if (!existsSync(dir)) break;
        }
      }
    },
  };

  try {
    for (let tries = 0; ; tries++) {
      try {
        await fetch(`${base}/status`);
        break;
      } catch (error) {
        if (tries > 50) throw error;
        await pause(100);
      }
    }
    const created = await app.call("POST", "/session", {
      capabilities: { alwaysMatch: { "tauri:options": { application: appBinary } } },
    });
    session = created.sessionId;
    await app.waitFor('return document.getElementById("status").textContent', "3 sample(s)");
  } catch (error) {
    await app.close();
    throw error;
  }
  return app;
}
