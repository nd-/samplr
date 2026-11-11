// The Settings window: shortcuts, the colour palette, the theme.

// The Settings window: for now, the palette. Each change is saved at once.
const settingsDialog = document.getElementById("settings");
const colorList = document.getElementById("color-list");

if (stored("infoFolded") === "true") setInfoFolded(true);

// The Settings window's list of shortcuts: keys, then what they do.
const shortcutList = [
  ["↑ ↓", "Browse the list and play"],
  ["Enter", "Play the selected sample again"],
  ["Space", "Pause or resume a loop, or a sample played with Loop on; play a one-shot again"],
  ["L", "Play in a loop, or no longer (the Play loop box)"],
  ["Esc", "Stop"],
  ["Ctrl+click", "Add a sample to the selection, or take it out"],
  ["Shift+click, Shift+↑ ↓", "Select from the last sample clicked"],
  ["Ctrl+A", "Select every sample of the list"],
  ["Ctrl+T", "Go to the tag box: Enter adds the tags typed, Backspace on an empty box removes the last one"],
  ["Ctrl+D", "Make the selection favorites, or no longer"],
  ["Ctrl+B", "Show or hide the collections"],
  ["Ctrl+I", "Show or hide the Info panel"],
  ["Del", "Take the selection out of the open collection"],
  ["Double-click", "Edit the tempo, the key or the type of a sample, or switch loop / one-shot"],
  ["Right-click", "Collections, detect the tempo, rename, move, convert or trash the selection"],
  ["Right-click the header", "Show or hide columns"],
  ["Drag a row", "Drop the selection into your DAW, or onto a collection"],
];

function renderShortcuts() {
  const items = [];
  for (const [keys, action] of shortcutList) {
    const term = document.createElement("dt");
    for (const [i, key] of shortcut(keys).split(", ").entries()) {
      if (i > 0) term.append(" ");
      const kbd = document.createElement("kbd");
      kbd.textContent = key;
      term.append(kbd);
    }
    const description = document.createElement("dd");
    description.textContent = action;
    items.push(term, description);
  }
  document.getElementById("shortcuts").replaceChildren(...items);
}
renderShortcuts();

function renderSettings() {
  const items = palette.map((color) => {
    const item = document.createElement("li");
    const picker = document.createElement("input");
    picker.type = "color";
    picker.value = color.hex;
    // The name is read only until the pencil is pressed, so that a stray
    // click or key does not rename a colour already used.
    const name = document.createElement("input");
    name.type = "text";
    name.value = color.name;
    name.spellcheck = false;
    name.readOnly = true;
    const rename = document.createElement("button");
    rename.type = "button";
    rename.textContent = "✎";
    rename.title = `Rename ${color.name}`;
    rename.addEventListener("click", () => editColorName(name));
    const save = async () => {
      try {
        await invoke("update_color", { id: color.id, name: name.value, hex: picker.value });
      } catch (error) {
        showError(error);
        return;
      }
      await loadColors();
    };
    // `change`: when the picker closes or the name box is left, not at
    // every step. A name still the one given automatically follows the
    // colour; one typed by hand stays.
    picker.addEventListener("change", () => {
      if (isAutomaticName(name.value, color.hex)) name.value = colorName(picker.value, color.id);
      save();
    });
    name.addEventListener("change", save);
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.title = `Delete ${color.name}`;
    remove.addEventListener("click", () => deleteColor(color));
    item.append(picker, name, rename, remove);
    return item;
  });
  colorList.replaceChildren(...items);
}

// Enter or leaving the box saves (its `change` event), Esc puts the name
// back, without closing the Settings window.
function editColorName(name) {
  const before = name.value;
  name.readOnly = false;
  name.focus();
  name.select();
  function onKey(event) {
    if (event.key === "Enter") name.blur();
    else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      name.value = before;
      name.blur();
    }
  }
  name.addEventListener("keydown", onKey);
  name.addEventListener(
    "blur",
    () => {
      name.readOnly = true;
      name.removeEventListener("keydown", onKey);
    },
    { once: true },
  );
}

async function deleteColor(color) {
  try {
    const [tags, collectionCount] = await invoke("color_usage", { id: color.id });
    const usage =
      tags + collectionCount > 0
        ? `${tags} tag(s) and ${collectionCount} collection(s) use it; ` +
          "they will be left without colour."
        : "No tag or collection uses it.";
    const confirmed = await confirmAction(
      `Delete the colour ${color.name}?\n\n${usage}`,
      "Delete colour",
    );
    if (!confirmed) return;
    await invoke("delete_color", { id: color.id });
  } catch (error) {
    showError(error);
    return;
  }
  await loadColors();
  await loadCollections();
  renderSettings();
}

// Hue (0 to 360), saturation and lightness (0 to 1) of a `#rrggbb`.
function hsl(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return [0, 0, l];
  const s = d / (1 - Math.abs(2 * l - 1));
  let h;
  if (max === r) h = ((g - b) / d) % 6;
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  return [(h * 60 + 360) % 360, s, l];
}

// The upper bound of each hue's range, in degrees.
const hueNames = [
  [12, "Red"], [35, "Orange"], [60, "Amber"], [100, "Lime"], [162, "Green"],
  [190, "Teal"], [200, "Cyan"], [218, "Blue"], [255, "Indigo"], [290, "Purple"],
  [340, "Pink"], [360, "Red"],
];

// A name for a colour, after its hue and lightness ("Dark teal", "Grey"),
// numbered when the palette already has it ("Teal 2"). `ownId`: the colour
// being renamed, whose own name does not count.
function colorName(hex, ownId = null) {
  const [h, s, l] = hsl(hex);
  let base;
  if (l > 0.94) base = "White";
  else if (l < 0.08) base = "Black";
  else if (s < 0.15) base = "Grey";
  else base = hueNames.find(([upper]) => h < upper)[1];
  if (base === "Grey" || hueNames.some(([, n]) => n === base)) {
    if (l > 0.75) base = `Light ${base.toLowerCase()}`;
    else if (l < 0.3) base = `Dark ${base.toLowerCase()}`;
  }
  const taken = new Set(palette.filter((c) => c.id !== ownId).map((c) => c.name));
  let name = base;
  for (let n = 2; taken.has(name); n++) name = `${base} ${n}`;
  return name;
}

// True when `name` is what `colorName` gives for `hex`, numbered or not:
// the user has not renamed the colour.
function isAutomaticName(name, hex) {
  const unnumbered = name.replace(/ \d+$/, "");
  return colorName(hex, -1).replace(/ \d+$/, "") === unnumbered;
}

// A new colour: the hue farthest from those of the palette, so that it can
// be told apart from them.
function newColorHex() {
  const hues = palette.map((c) => hsl(c.hex)).filter(([, s]) => s >= 0.15).map(([h]) => h);
  let best = 0;
  let bestDistance = -1;
  for (let hue = 0; hue < 360; hue += 5) {
    const distance = Math.min(
      360,
      ...hues.map((other) => Math.min(Math.abs(hue - other), 360 - Math.abs(hue - other))),
    );
    if (distance > bestDistance) [best, bestDistance] = [hue, distance];
  }
  return hslToHex(best, 0.7, 0.5);
}

function hslToHex(h, s, l) {
  const k = (n) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n) => l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
  return `#${[0, 8, 4].map((n) => Math.round(f(n) * 255).toString(16).padStart(2, "0")).join("")}`;
}

document.getElementById("add-color").addEventListener("click", async () => {
  const hex = newColorHex();
  try {
    await invoke("add_color", { name: colorName(hex), hex });
  } catch (error) {
    showError(error);
    return;
  }
  await loadColors();
  renderSettings();
  const added = colorList.lastElementChild?.querySelector('input[type="text"]');
  if (added) editColorName(added);
});

// The theme: a `data-theme` on <html>, read by the stylesheet; applied at
// startup by theme.js.
const themeSelect = document.getElementById("theme");
themeSelect.value = document.documentElement.dataset.theme ?? "system";
// A theme remembered from an older samplr may no longer exist: back to System.
if (themeSelect.value === "") {
  delete document.documentElement.dataset.theme;
  themeSelect.value = "system";
}

themeSelect.addEventListener("change", () => {
  const theme = themeSelect.value;
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  store("theme", theme);
  // The waveform is drawn in the accent colour.
  drawWaveform();
});

const askCheckbox = document.getElementById("ask-before-deleting");
askCheckbox.checked = askBeforeDeleting;
askCheckbox.addEventListener("change", () => {
  askBeforeDeleting = askCheckbox.checked;
  store("askBeforeDeleting", askBeforeDeleting);
});

closeOnPressOutside(settingsDialog);
closeOnPressOutside(foldersDialog);

document.getElementById("show-settings").addEventListener("click", () => {
  renderSettings();
  settingsDialog.showModal();
});
