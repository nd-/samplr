// The folders of the library: the Folders window, removing a folder,
// adding one with "+ Folder", and the Splice folders offered once.
const { test, expect, openFakeLibrary, calls, name } = require("./helpers");

openFakeLibrary();

const dialog = (page) => page.locator("#folders");
const folders = (page) => page.locator("#folder-list li");
const folder = (page, path) => folders(page).filter({ hasText: path });
const asked = (page) => page.evaluate(() => window.__asked);
const set = (page, values) => page.evaluate((values) => Object.assign(window, values), values);

async function openFolders(page) {
  await page.locator("#show-folders").click();
  await expect(dialog(page)).toBeVisible();
}

test("the scanned folders, their samples, and a disk not found", async ({ page }) => {
  await openFolders(page);
  await expect(folders(page)).toHaveCount(2);
  await expect(folder(page, "/lib").locator(".count")).toHaveText("50 sample(s)");
  await expect(folder(page, "/Volumes/Old disk").locator(".missing")).toHaveText(" (not found)");
  await expect(folder(page, "/lib").locator(".missing")).toHaveCount(0);
});

test("removing a folder is confirmed, its samples leave the list", async ({ page }) => {
  await openFolders(page);
  await set(page, { __answer: false });
  await folder(page, "/lib").locator("button").click();
  await expect.poll(() => asked(page)).toHaveLength(1);
  expect((await asked(page))[0]).toContain("Its 50 sample(s) leave the list. Their tags are kept");
  expect(await calls(page, "remove_folder")).toHaveLength(0);

  await set(page, { __answer: true });
  await folder(page, "/lib").locator("button").click();
  await expect(folder(page, "/lib")).toHaveCount(0);
  await expect(page.locator("#status")).toHaveText("Folder removed, 50 sample(s) hidden.");
  await expect(page.locator("#samples tr[data-index]")).toHaveCount(0);

  // The last one gone, the window says how to add one.
  await folder(page, "/Volumes/Old disk").locator("button").click();
  await expect(folders(page)).toHaveText(["No folder yet: add one with + Folder."]);
});

test("the list's keys are off while the window is open", async ({ page }) => {
  await openFolders(page);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press(" ");
  await expect(page.locator("#samples tr.selected")).toHaveCount(0);
  expect(await calls(page, "play")).toHaveLength(0);
  // Esc closes the window, and the keys work again.
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toBeHidden();
  await name(page, 0).click();
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#samples tr.selected")).toHaveCount(1);
});

test("a press outside closes the window", async ({ page }) => {
  await openFolders(page);
  await page.mouse.click(5, 5);
  await expect(dialog(page)).toBeHidden();
});

test("a folder added is scanned, its progress shown", async ({ page }) => {
  await set(page, { __chosenFolder: "/new" });
  // The scan answers late: the progress can be seen meanwhile.
  await page.evaluate(() => {
    window.__delay = (command) => (command === "add_folder" ? 400 : 0);
  });
  await page.locator("#add-folder").click();
  await expect(page.locator("#status")).toHaveText("Scanning /new… 3 / 3");
  await expect(page.locator("#scan-bar")).toBeVisible();
  await expect(page.locator("#add-folder")).toBeDisabled();

  await expect(page.locator("#status")).toHaveText("3 sample(s) saved, 0 unreadable.");
  await expect(page.locator("#scan-bar")).toBeHidden();
  await expect(page.locator("#add-folder")).toBeEnabled();
  await openFolders(page);
  await expect(folder(page, "/new").locator(".count")).toHaveText("3 sample(s)");
});

test("a folder chooser cancelled scans nothing", async ({ page }) => {
  await page.locator("#add-folder").click();
  await expect(page.locator("#add-folder")).toBeEnabled();
  expect(await calls(page, "add_folder")).toHaveLength(0);
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
});

test("the Splice folder is offered once, at the first launch where one is there", async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.__splice = ["/home/me/Splice/sounds"];
  });
  await page.reload();
  await expect(page.locator("#status")).toHaveText("3 sample(s) saved, 0 unreadable.");
  expect(await asked(page)).toHaveLength(1);
  expect((await asked(page))[0]).toContain("/home/me/Splice/sounds");
  expect((await calls(page, "add_folder")).at(-1).folder).toBe("/home/me/Splice/sounds");

  // Asked once, whatever the answer.
  await page.reload();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  expect(await asked(page)).toHaveLength(0);
});
