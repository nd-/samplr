// The cells of a row and their editing (tempo, key, loop, type), the type
// picker, and the colour menus.

// The palette, and the colours of the tags (by name).
let palette = [];
let tagColorIds = new Map();

function colorHex(id) {
  return palette.find((color) => color.id === id)?.hex ?? null;
}

function tagColor(tag) {
  return colorHex(tagColorIds.get(tag) ?? null);
}

// Black or white text, whichever reads better on `hex` (perceived
// brightness of the colour).
function textOn(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  return r * 0.299 + g * 0.587 + b * 0.114 > 150 ? "#1b1b1b" : "#ffffff";
}

// Gives an element a coloured background, or takes it away (`null`).
function paint(element, hex) {
  element.style.background = hex ?? "";
  element.style.color = hex ? textOn(hex) : "";
}

function dot(hex) {
  const span = document.createElement("span");
  span.className = "dot";
  span.style.background = hex;
  return span;
}

// The tempo of a row: as written in the name or in the file's ACID chunk,
// or "≈ 100" when estimated from the sample's length or its sound.
function bpmCell(sample) {
  if (sample.bpm === null) return cell("", "number bpm");
  if (tempoEstimated(sample)) {
    const td = cell(`≈ ${sample.bpm}`, "number bpm estimated");
    td.title = `${tempoOrigin(sample)[0].toUpperCase()}${tempoOrigin(sample).slice(1)}. Double-click to set it`;
    return td;
  }
  const td = cell(String(sample.bpm), "number bpm");
  if (sample.bpm_source === "file") td.title = "From the file (its ACID chunk: beats and length)";
  return td;
}

// Loudness in LUFS, one decimal; the peak in the tooltip. Empty until the
// waveform (which measures it) is computed.
function loudnessCell(sample) {
  const td = cell(sample.lufs === null ? "" : sample.lufs.toFixed(1), "number");
  td.title = loudnessText(sample);
  return td;
}

// "-12.9 LUFS, peak -0.2 dBFS"; for a sound too short for LUFS, the peak.
function loudnessText(sample) {
  const parts = [];
  if (sample.lufs !== null) parts.push(`${sample.lufs.toFixed(1)} LUFS`);
  if (sample.peak_db !== null) parts.push(`peak ${sample.peak_db.toFixed(1)} dBFS`);
  return parts.join(", ");
}

// Loop or one-shot: ✓ or —, greyed when only guessed from the tempo.
const loopOrigins = {
  file: "from the file's ACID chunk",
  name: "from the words of its name or folders",
  length: "guessed: a loop when it has a tempo",
  manual: "set by hand",
};

function loopCell(sample) {
  const guess = sample.loop_source === "length";
  const td = cell(sample.looped ? "✓" : "—", guess ? "loop estimated" : "loop");
  td.title = `${sample.looped ? "Loop" : "One-shot"}, ${loopOrigins[sample.loop_source] ?? ""}. Double-click to change`;
  return td;
}

// A double-click on Loop switches it, for the whole selection when the row
// is in it. A value that is the one samplr works out by itself is not kept
// as set by hand: switching twice goes back to automatic.
async function switchLoop(index) {
  const sample = samples[index];
  const wanted = !sample.looped;
  const chosen = samplesFor(sample);
  // Worked out by itself: the value shown, unless it was set by hand, in
  // which case it is the other one (only a value differing from it is kept).
  const automatic = (s) => (s.loop_source === "manual" ? !s.looped : s.looped);
  const backToAuto = chosen.filter((s) => automatic(s) === wanted).map((s) => s.path);
  const byHand = chosen.filter((s) => automatic(s) !== wanted).map((s) => s.path);
  try {
    if (backToAuto.length > 0) await invoke("set_loop", { paths: backToAuto, looped: null });
    if (byHand.length > 0) await invoke("set_loop", { paths: byHand, looped: wanted });
  } catch (error) {
    showError(error);
  }
  await refresh({ stopIfGone: false });
  flash(rowsOf(chosen.map((s) => s.path)));
}

tbody.addEventListener("dblclick", (event) => {
  const index = rowIndex(event);
  if (index >= 0 && event.target.closest("td.loop")) {
    switchLoop(index);
    return;
  }
  if (index >= 0 && event.target.closest("td.type")) {
    editType(event.clientX, event.clientY, index);
    return;
  }
  const td = event.target.closest("td.bpm, td.key");
  if (index < 0 || !td || td.querySelector("input")) return;
  editCell(index, td, td.classList.contains("bpm") ? "set_tempo" : "set_key");
});

// Tempo and key by hand: a double-click on the cell edits it, for the whole
// selection when the row is in it. Enter or leaving the box saves, Esc
// cancels; empty or "-" means none (a wrong guess). Back to the value worked
// out through the row menu.
function editCell(index, td, command) {
  const sample = samples[index];
  const initial = String((command === "set_tempo" ? sample.bpm : sample.key) ?? "");
  const input = document.createElement("input");
  input.type = "text";
  input.value = initial;
  input.placeholder = "none";
  input.spellcheck = false;
  input.title =
    "Enter to save, Esc to cancel. Empty: none. Back to automatic: right-click, Detect BPM… or Automatic key";
  td.replaceChildren(input);
  input.focus();
  input.select();

  // A value refused after Enter keeps the box open, outlined in red, to be
  // corrected; refused on leaving the box, the old value comes back.
  let done = false;
  async function finish(save, leaving = false) {
    if (done) return;
    done = true;
    const value = input.value.trim();
    if (!save || value === initial) {
      // The row may have been drawn again meanwhile (scrolling).
      if (td.isConnected) td.replaceWith(command === "set_tempo" ? bpmCell(sample) : keyCell(sample));
      refreshIfWaiting();
      return;
    }
    const paths = pathsFor(sample);
    try {
      await invoke(command, { paths, value });
    } catch (error) {
      showError(error);
      if (!td.isConnected) return;
      if (leaving) {
        td.replaceWith(command === "set_tempo" ? bpmCell(sample) : keyCell(sample));
        refreshIfWaiting();
        return;
      }
      done = false;
      input.classList.add("invalid");
      input.title = String(error);
      input.select();
      return;
    }
    await refresh({ stopIfGone: false });
    flash(rowsOf(paths));
  }
  input.addEventListener("keydown", (event) => {
    // The list's keys (arrows, Enter, Esc, Del) must not react.
    event.stopPropagation();
    if (event.key === "Enter") finish(true);
    else if (event.key === "Escape") finish(false);
  });
  input.addEventListener("input", () => input.classList.remove("invalid"));
  input.addEventListener("blur", () => finish(true, true));
}

// The types of sound, from `sound_types`: families, each with its subtypes
// (`{ id, label, subtypes: [{ id, label }] }`). A sample's type is
// `family` or `family/subtype`.
let soundFamilies = [];

// "Drums › kick", "Brass & Woodwinds", or "" for no type.
function typeName(value) {
  if (!value) return "";
  const [familyId, subtypeId] = value.split("/");
  const family = soundFamilies.find((f) => f.id === familyId);
  if (!family) return value;
  const subtype = family.subtypes.find((s) => s.id === subtypeId);
  return subtype ? `${family.label} › ${subtype.label}` : family.label;
}

function typeLabel(sample) {
  return typeName(sample.sound_type);
}

const typeOrigins = {
  name: "from the words of its name or folders",
  manual: "set by hand",
};

function typeCell(sample) {
  const td = cell(typeLabel(sample), sample.type_source === "name" ? "type estimated" : "type");
  td.title = `${typeLabel(sample) || "No type"}, ${typeOrigins[sample.type_source] ?? "nothing in its name or folders says it"}. Double-click to change`;
  return td;
}

// The type picker: a search box, the types used lately, then the families
// on the left and the subtypes of the one pointed at on the right. Typing
// lists the families and subtypes matching the text instead. Used to set a
// sample's type and to filter the list by type.
const typePicker = document.getElementById("type-picker");
const typeSearch = document.getElementById("type-search");
const typeRecent = document.getElementById("type-recent");
const typeFamilies = document.getElementById("type-families");
const typeSubtypes = document.getElementById("type-subtypes");
const typeResults = document.getElementById("type-results");
// The open picker: what is chosen now, the entries above the families
// ([label, value]), what to do with the choice, the family whose subtypes
// show, and the entry the keys are on (`pane`: "left", "right" or
// "results").
let picker = null;

function recentTypes() {
  const saved = storedJson("recentTypes", []);
  return Array.isArray(saved) ? saved.filter((value) => typeName(value) !== value) : [];
}

function rememberType(value) {
  const recent = [value, ...recentTypes().filter((v) => v !== value)].slice(0, 5);
  store("recentTypes", JSON.stringify(recent));
}

// Case and accents ignored.
function folded(text) {
  return text.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
}

function openTypePicker(x, y, { current, extra, choose }) {
  closeContextMenu();
  const familyId = current?.split("/")[0];
  picker = {
    current,
    extra,
    choose,
    family: soundFamilies.find((f) => f.id === familyId) ?? soundFamilies[0],
    pane: "left",
    index: 0,
  };
  typeSearch.value = "";
  typePicker.hidden = false;
  renderTypePicker();
  placeInWindow(typePicker, x, y);
  typeSearch.focus();
}

function closeTypePicker() {
  typePicker.hidden = true;
  picker = null;
  refreshIfWaiting();
}

function pickType(value) {
  const { choose } = picker;
  if (typeName(value) !== value && value) rememberType(value);
  closeTypePicker();
  choose(value);
}

function pickerButton(label, value, active) {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = value === picker.current ? `✓ ${label}` : label;
  button.classList.toggle("active", active);
  button.addEventListener("click", () => pickType(value));
  return button;
}

// The entries of the left pane: the extra ones, then the families.
function leftEntries() {
  return [
    ...picker.extra.map(([label, value]) => ({ label, value })),
    ...soundFamilies.map((family) => ({ label: family.label, value: family.id, family })),
  ];
}

function searchedTypes(text) {
  const wanted = folded(text);
  const found = [];
  for (const family of soundFamilies) {
    if (folded(family.label).includes(wanted)) found.push(family.id);
    for (const subtype of family.subtypes) {
      if (folded(subtype.label).includes(wanted)) found.push(`${family.id}/${subtype.id}`);
    }
  }
  return found;
}

function renderTypePicker() {
  const text = typeSearch.value.trim();
  const searching = text !== "";
  typeResults.hidden = !searching;
  typeFamilies.parentElement.hidden = searching;
  typeRecent.hidden = searching;
  if (searching) {
    const found = searchedTypes(text);
    picker.pane = "results";
    picker.index = Math.min(picker.index, Math.max(0, found.length - 1));
    typeResults.replaceChildren(
      ...found.map((value, i) => pickerButton(typeName(value), value, i === picker.index)),
    );
    if (found.length === 0) {
      typeResults.append(menuLabel("No type matches"));
    }
    typeResults.querySelector(".active")?.scrollIntoView({ block: "nearest" });
    return;
  }
  if (picker.pane === "results") {
    picker.pane = "left";
    picker.index = 0;
  }
  const recent = recentTypes();
  typeRecent.replaceChildren();
  if (recent.length > 0) {
    typeRecent.append(menuLabel("Recent", "span"), ...recent.map((value) => pickerButton(typeName(value), value, false)));
  }
  typeRecent.hidden = recent.length === 0;
  const left = leftEntries().map((entry, i) => {
    const button = pickerButton(
      entry.family?.subtypes.length ? `${entry.label} ›` : entry.label,
      entry.value,
      picker.pane === "left" && i === picker.index,
    );
    if (entry.family === picker.family) button.classList.add("open");
    if (entry.family) {
      // Only the right pane is drawn again: replacing the button under the
      // pointer would lose the click that follows.
      button.addEventListener("mouseenter", () => {
        picker.family = entry.family;
        for (const other of typeFamilies.querySelectorAll("button")) {
          other.classList.toggle("open", other === button);
        }
        renderSubtypes();
      });
    }
    return button;
  });
  left.splice(picker.extra.length, 0, document.createElement("hr"));
  typeFamilies.replaceChildren(...left);
  renderSubtypes();
  typePicker.querySelector(".active")?.scrollIntoView({ block: "nearest" });
}

function renderSubtypes() {
  const family = picker.family;
  typeSubtypes.replaceChildren(
    ...(family?.subtypes ?? []).map((subtype, i) =>
      pickerButton(
        subtype.label,
        `${family.id}/${subtype.id}`,
        picker.pane === "right" && i === picker.index,
      ),
    ),
  );
}

typeSearch.addEventListener("input", () => {
  if (picker) picker.index = 0;
  renderTypePicker();
});

typeSearch.addEventListener("keydown", (event) => {
  // The list's keys must not react while choosing.
  event.stopPropagation();
  if (!picker) return;
  const move = { ArrowDown: 1, ArrowUp: -1 }[event.key];
  if (event.key === "Escape") {
    closeTypePicker();
  } else if (move) {
    const count =
      picker.pane === "results"
        ? typeResults.querySelectorAll("button").length
        : picker.pane === "left"
          ? leftEntries().length
          : (picker.family?.subtypes.length ?? 0);
    picker.index = Math.max(0, Math.min(count - 1, picker.index + move));
    if (picker.pane === "left") picker.family = leftEntries()[picker.index].family ?? picker.family;
    renderTypePicker();
  } else if (event.key === "ArrowRight" && picker.pane === "left" && picker.family?.subtypes.length) {
    const entry = leftEntries()[picker.index];
    if (!entry.family) return;
    picker.pane = "right";
    picker.index = 0;
    renderTypePicker();
  } else if (event.key === "ArrowLeft" && picker.pane === "right") {
    picker.pane = "left";
    picker.index = leftEntries().findIndex((entry) => entry.family === picker.family);
    renderTypePicker();
  } else if (event.key === "Enter") {
    // The hidden panes keep their own active entry: only the one shown counts.
    const shown = picker.pane === "results" ? typeResults : typeFamilies.parentElement;
    shown.querySelector("button.active")?.click();
  } else {
    return;
  }
  event.preventDefault();
});

document.addEventListener("mousedown", (event) => {
  if (picker && !typePicker.contains(event.target)) closeTypePicker();
});

// A double-click on a Type cell: the picker sets the type of the sample, or
// of the whole selection when the row is in it.
function editType(x, y, index) {
  const sample = samples[index];
  const paths = pathsFor(sample);
  const current = sample.type_source === "manual" ? (sample.sound_type ?? "-") : "";
  openTypePicker(x, y, {
    current,
    extra: [
      ["Automatic", ""],
      ["No type", "-"],
    ],
    choose: async (value) => {
      try {
        await invoke("set_sound_type", { paths, value });
      } catch (error) {
        showError(error);
        return;
      }
      await refresh({ stopIfGone: false });
      flash(rowsOf(paths));
    },
  });
}

// The Type button of the header: the same picker, writing `type:…` in the
// search box.
const typeFilterButton = document.getElementById("type-filter");

function searchedType() {
  const word = searchBox.value.split(/\s+/).find((w) => w.toLowerCase().startsWith("type:"));
  return word ? word.slice(5).toLowerCase() : "";
}

function updateTypeFilterButton() {
  const value = searchedType();
  const name = value ? typeName(value) || value : "";
  typeFilterButton.textContent = name ? `Type: ${name}` : "Type";
  typeFilterButton.setAttribute("aria-pressed", String(value !== ""));
}

typeFilterButton.addEventListener("click", () => {
  const { left, bottom } = typeFilterButton.getBoundingClientRect();
  openTypePicker(left, bottom + 4, {
    current: searchedType(),
    extra: [["Any type", ""]],
    choose: (value) => {
      const kept = searchBox.value
        .split(/\s+/)
        .filter((word) => word !== "" && !word.toLowerCase().startsWith("type:"));
      if (value) kept.push(`type:${value}`);
      searchBox.value = kept.join(" ");
      search();
    },
  });
});

function keyCell(sample) {
  return cell(sample.key ?? "", "key");
}

// The names of collections, by `id`, in the order of the left column.
function collectionNames(ids) {
  return collections.filter((c) => ids.includes(c.id)).map((c) => c.name);
}

// The Collections cell of a row: each name after its colour's dot.
function collectionsCell(ids) {
  const td = cell("", "collections");
  collections
    .filter((collection) => ids.includes(collection.id))
    .forEach((collection, i) => {
      if (i > 0) td.append(", ");
      const hex = colorHex(collection.color_id);
      if (hex) td.append(dot(hex));
      td.append(collection.name);
    });
  return td;
}

// The Tags cell of a row: coloured tags as chips, the others as text.
function tagsCell(tags) {
  const td = cell("", "tags");
  tags.forEach((tag, i) => {
    if (i > 0) td.append(" ");
    const hex = tagColor(tag);
    if (hex) {
      const chip = document.createElement("span");
      chip.className = "chip";
      chip.textContent = tag;
      paint(chip, hex);
      td.append(chip);
    } else {
      td.append(tag);
    }
  });
  return td;
}

async function loadColors() {
  try {
    const [colors, tagColors] = await Promise.all([invoke("colors"), invoke("tag_colors")]);
    palette = colors;
    tagColorIds = new Map(tagColors);
  } catch (error) {
    showError(error);
    return;
  }
  drawRows();
  renderTags();
  renderCollections();
  renderTagList();
}

// The palette's colours to choose from, plus "No colour"; `current` is
// marked. Used for a tag and for a collection.
function openColorMenu(x, y, current, choose) {
  // Shares the menu with the rows: a row's menu still waiting for its answer
  // must not replace this one.
  menuOpening += 1;
  const items = [];
  const none = menuButton(current === null ? "✓ No colour" : "No colour", () => choose(null));
  items.push(none);
  for (const color of palette) {
    const label = color.id === current ? `✓ ${color.name}` : color.name;
    items.push(menuButton(label, () => choose(color.id), false, color.hex));
  }
  showMenu(items, x, y);
}

// The colour menu of a tag, in the tag bar or the Tags box.
function openTagColorMenu(event, tag) {
  event.preventDefault();
  openColorMenu(event.clientX, event.clientY, tagColorIds.get(tag) ?? null, async (color) => {
    await invoke("set_tag_color", { tag, color });
    await loadColors();
  });
}
