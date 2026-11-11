// The type picker: setting a sample's type with the mouse, the keys or a
// search, the types used lately, and the header's Type filter.
const { test, expect, openFakeLibrary, calls, row, name } = require("./helpers");

openFakeLibrary();

// In the fake library: a kick, typed from its name, and a snare, untyped.
const KICK = 0;
const SNARE = 12;

const picker = (page) => page.locator("#type-picker");
const families = (page) => page.locator("#type-families button");
const subtypes = (page) => page.locator("#type-subtypes button");
const typeCell = (page, index) => row(page, index).locator("td.type");
const lastType = async (page) => (await calls(page, "set_sound_type")).at(-1);

test("a double-click opens the picker on the sample's family", async ({ page }) => {
  await typeCell(page, KICK).dblclick();
  await expect(picker(page)).toBeVisible();
  await expect(page.locator("#type-search")).toBeFocused();
  // Its type worked out, not set by hand: Automatic is the current choice.
  await expect(families(page)).toHaveText([
    "✓ Automatic",
    "No type",
    "Drums ›",
    "Bass ›",
    "Guitar ›",
  ]);
  await expect(families(page).filter({ hasText: "Drums" })).toHaveClass(/open/);
  await expect(subtypes(page)).toHaveText(["Kick", "Snare"]);
});

test("a subtype clicked is set, and offered among the recent ones", async ({ page }) => {
  await typeCell(page, SNARE).dblclick();
  await subtypes(page).filter({ hasText: "snare" }).click();
  await expect(picker(page)).toBeHidden();
  await expect(typeCell(page, SNARE)).toHaveText("Drums › Snare");
  // Set by hand: no longer greyed as worked out.
  await expect(typeCell(page, SNARE)).not.toHaveClass(/estimated/);
  expect(await lastType(page)).toEqual({ paths: ["/lib/Drums/Snare 1.wav"], value: "drums/snare" });

  await typeCell(page, SNARE + 1).dblclick();
  await expect(page.locator("#type-recent button")).toHaveText(["Drums › Snare"]);
});

test("pointing at a family shows its subtypes", async ({ page }) => {
  await typeCell(page, SNARE).dblclick();
  await families(page).filter({ hasText: "Guitar" }).hover();
  await expect(subtypes(page)).toHaveText(["Acoustic", "Electric"]);
  await subtypes(page).filter({ hasText: "acoustic" }).click();
  expect((await lastType(page)).value).toBe("guitar/acoustic");
});

test("the arrow keys and Enter choose", async ({ page }) => {
  await typeCell(page, SNARE).dblclick();
  // Automatic, No type, then Drums; → into its subtypes, ↓ to the second.
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(families(page).filter({ hasText: "Drums" })).toHaveClass(/active/);
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await expect(subtypes(page).filter({ hasText: "snare" })).toHaveClass(/active/);
  await page.keyboard.press("Enter");
  expect((await lastType(page)).value).toBe("drums/snare");
});

test("typing lists the matching types, in every family", async ({ page }) => {
  await typeCell(page, SNARE).dblclick();
  await page.keyboard.type("ELEC");
  const results = page.locator("#type-results button");
  await expect(results).toHaveText(["Bass › Electric", "Guitar › Electric"]);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  expect((await lastType(page)).value).toBe("guitar/electric");

  await typeCell(page, SNARE + 1).dblclick();
  await page.keyboard.type("theremin");
  await expect(page.locator("#type-results")).toHaveText("No type matches");
});

test("Esc or a press outside closes without choosing", async ({ page }) => {
  await typeCell(page, SNARE).dblclick();
  await page.keyboard.press("Escape");
  await expect(picker(page)).toBeHidden();
  await typeCell(page, SNARE).dblclick();
  await page.locator("#status").click();
  await expect(picker(page)).toBeHidden();
  expect(await calls(page, "set_sound_type")).toHaveLength(0);
  // The list's keys work again: Esc in the picker did not stop a sound.
  expect(await calls(page, "stop")).toHaveLength(0);
});

test("No type, then Automatic back to the type worked out", async ({ page }) => {
  await typeCell(page, KICK).dblclick();
  await families(page).filter({ hasText: "No type" }).click();
  await expect(typeCell(page, KICK)).toHaveText("");
  expect((await lastType(page)).value).toBe("-");

  await typeCell(page, KICK).dblclick();
  // The current choice is marked.
  await expect(families(page).filter({ hasText: "No type" })).toHaveText("✓ No type");
  await families(page).filter({ hasText: "Automatic" }).click();
  await expect(typeCell(page, KICK)).toHaveText("Drums › Kick");
  await expect(typeCell(page, KICK)).toHaveClass(/estimated/);
});

test("the whole selection gets the type when the row is in it", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, SNARE).click();
  await name(page, SNARE + 2).click({ modifiers: ["Shift"] });
  await typeCell(page, SNARE + 1).dblclick();
  await subtypes(page).filter({ hasText: "snare" }).click();
  expect((await lastType(page)).paths).toHaveLength(3);
  await expect(typeCell(page, SNARE + 2)).toHaveText("Drums › Snare");
  await expect(page.locator("#samples tr.selected")).toHaveCount(3);
});

test("a single click on a selected row's cell still selects that row alone", async ({ page }) => {
  await page.locator("#autoplay").uncheck();
  await name(page, SNARE).click();
  await name(page, SNARE + 2).click({ modifiers: ["Shift"] });
  await typeCell(page, SNARE + 1).click();
  // Once the double-click time has passed.
  await expect(page.locator("#samples tr.selected")).toHaveCount(1);
  await expect(row(page, SNARE + 1)).toHaveClass(/selected/);
});

test("the header's Type button filters the list", async ({ page }) => {
  const button = page.locator("#type-filter");
  await button.click();
  await expect(families(page)).toHaveText(["✓ Any type", "Drums ›", "Bass ›", "Guitar ›"]);
  await families(page).filter({ hasText: "Drums" }).click();
  await expect(page.locator("#search")).toHaveValue("type:drums");
  await expect(button).toHaveText("Type: Drums");
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#status")).toHaveText("12 sample(s)");

  // The other words of the search are kept.
  await page.locator("#search").fill("kick 1 type:drums");
  await button.click();
  await families(page).filter({ hasText: "Any type" }).click();
  await expect(page.locator("#search")).toHaveValue("kick 1");
  await expect(button).toHaveText("Type");
  await expect(button).toHaveAttribute("aria-pressed", "false");
});
