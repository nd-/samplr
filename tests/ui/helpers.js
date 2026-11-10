// Shared by the test files: the page opened on the fake library, and ways
// to look at it.
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const { test, expect } = require("@playwright/test");

const pageUrl = pathToFileURL(path.join(__dirname, "../../ui/index.html")).href;

// Each test of the calling file starts on the page showing the fake library
// (50 samples), and ends with no error shown: every command the page used
// was answered. Called by each file: a module is loaded only once per
// worker, hooks registered at its loading would only reach the first file.
function openFakeLibrary() {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript({ path: path.join(__dirname, "fake-tauri.js") });
    await page.goto(pageUrl);
    await expect(page.locator("#status")).toHaveText("50 sample(s)");
  });

  test.afterEach(async ({ page }) => {
    await expect(page.locator("#error-box")).toBeHidden();
  });
}

// The commands the page sent, by name.
async function calls(page, command) {
  const all = await page.evaluate(() => window.__calls);
  return all.filter((call) => call.command === command).map((call) => call.args);
}

const row = (page, index) => page.locator(`#samples tr[data-index="${index}"]`);
const name = (page, index) => row(page, index).locator("td").nth(1);

module.exports = { test, expect, openFakeLibrary, calls, row, name };
