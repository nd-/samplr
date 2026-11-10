// Collections: creating, adding from the row menu, opening, taking samples
// out, deleting.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

const list = (page) => page.locator("#collection-list");
const item = (page, label) => list(page).locator("li", { hasText: label });
const menu = (page) => page.locator("#context-menu");

async function create(page, label) {
  await page.locator("#collection-name").fill(label);
  await page.locator("#collection-name").press("Enter");
  await expect(item(page, label)).toBeVisible();
}

// Selects rows `first` to `last`, without playing them.
async function select(page, first, last) {
  await page.locator("#autoplay").uncheck();
  await name(page, first).click();
  if (last !== first) await name(page, last).click({ modifiers: ["Shift"] });
}

test("a collection is created empty, its name box emptied", async ({ page }) => {
  await create(page, "  Heavy kicks ");
  await expect(item(page, "Heavy kicks").locator(".count")).toHaveText("0");
  await expect(page.locator("#collection-name")).toHaveValue("");
  expect((await calls(page, "create_collection")).at(-1).name).toBe("Heavy kicks");
});

test("a name already taken is refused, and kept in the box", async ({ page }) => {
  await create(page, "Set");
  await page.locator("#collection-name").fill("Set");
  await page.locator("#collection-name").press("Enter");
  await expect(page.locator("#error-text")).toContainText("already exists");
  await expect(page.locator("#collection-name")).toHaveValue("Set");
  await page.locator("#error-close").click();
  await expect(list(page).locator("li", { hasText: "Set" })).toHaveCount(1);
});

test("the selection added from the row menu, which stays open", async ({ page }) => {
  await create(page, "Set");
  await select(page, 0, 2);
  await name(page, 1).click({ button: "right" });
  await menu(page).locator("button", { hasText: "Set" }).click();

  await expect(item(page, "Set").locator(".count")).toHaveText("3");
  for (const index of [0, 1, 2]) {
    await expect(row(page, index).locator("td.collections")).toHaveText("Set");
  }
  // Still open, the collection now marked as holding them, with a × to take
  // them out.
  await expect(menu(page)).toBeVisible();
  await expect(menu(page).locator("button:disabled", { hasText: "✓ Set" })).toBeVisible();
  await menu(page).locator("button.remove-from").click();
  await expect(item(page, "Set").locator(".count")).toHaveText("0");
});

test("a new collection named in the row menu gets the sample", async ({ page }) => {
  await select(page, 4, 4);
  await name(page, 4).click({ button: "right" });
  const box = menu(page).locator("input");
  await box.fill("Snares to check");
  await box.press("Enter");
  await expect(item(page, "Snares to check").locator(".count")).toHaveText("1");
  await expect(row(page, 4).locator("td.collections")).toHaveText("Snares to check");
  // Esc closes the menu.
  await page.keyboard.press("Escape");
  await expect(menu(page)).toBeHidden();
});

test("opening a collection shows its samples only, All samples brings the rest back", async ({
  page,
}) => {
  await create(page, "Set");
  await select(page, 10, 12);
  await name(page, 11).click({ button: "right" });
  await menu(page).locator("button", { hasText: "Set" }).click();
  await page.keyboard.press("Escape");

  await item(page, "Set").locator(".name").click();
  await expect(item(page, "Set")).toHaveClass(/active/);
  await expect(page.locator("#status")).toHaveText("3 sample(s)");
  expect((await calls(page, "search")).at(-1).collection).toBe(1);
  // In a collection, each row ends with a × to take it out.
  await expect(row(page, 0).locator("td.remove button")).toBeVisible();
  // A search applies within it.
  await page.locator("#search").fill("kick 12");
  await expect(page.locator("#status")).toHaveText("1 sample(s)");

  await page.locator("#search").fill("");
  await item(page, "All samples").locator(".name").click();
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect(row(page, 0).locator("td.remove")).toHaveCount(0);
});

test("samples taken out with a row's × or with Del", async ({ page }) => {
  await create(page, "Set");
  await select(page, 0, 3);
  await name(page, 0).click({ button: "right" });
  await menu(page).locator("button", { hasText: "Set" }).click();
  await page.keyboard.press("Escape");
  await item(page, "Set").locator(".name").click();
  await expect(page.locator("#status")).toHaveText("4 sample(s)");

  // The × takes out its own row only, selected or not.
  await row(page, 3).locator("td.remove button").click();
  await expect(page.locator("#status")).toHaveText("1 sample(s) removed from the collection.");
  await expect(item(page, "Set").locator(".count")).toHaveText("3");

  // Del takes out the selection.
  await name(page, 0).click();
  await name(page, 1).click({ modifiers: ["Shift"] });
  await page.keyboard.press("Delete");
  await expect(item(page, "Set").locator(".count")).toHaveText("1");
  await expect(page.locator("#samples tr[data-index]")).toHaveCount(1);
});

test("deleting a collection is confirmed, its samples stay", async ({ page }) => {
  await create(page, "Set");
  await select(page, 0, 1);
  await name(page, 0).click({ button: "right" });
  await menu(page).locator("button", { hasText: "Set" }).click();
  await page.keyboard.press("Escape");

  // Answered no: nothing happens.
  await page.evaluate(() => {
    window.__answer = false;
  });
  await item(page, "Set").locator("button").click();
  await expect(item(page, "Set")).toBeVisible();
  expect(await calls(page, "delete_collection")).toHaveLength(0);

  await page.evaluate(() => {
    window.__answer = true;
  });
  await item(page, "Set").locator("button").click();
  await expect(item(page, "Set")).toHaveCount(0);
  await expect(page.locator("#status")).toHaveText("50 sample(s)");
  await expect(row(page, 0).locator("td.collections")).toHaveText("");
});

test("collections are listed by name, case ignored", async ({ page }) => {
  for (const label of ["drums", "Ambient", "bass"]) await create(page, label);
  await expect(list(page).locator("li .name")).toHaveText([
    "All samples",
    "Ambient",
    "bass",
    "drums",
  ]);
});
