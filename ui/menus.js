// The row menu: collections, tempo and key, and the actions on the files
// themselves (rename, move, convert, trash).

// Right-click on a row: the row is selected (without playing it) and a menu
// offers the collections.
const contextMenu = document.getElementById("context-menu");

// `keepOpen`: the menu stays open after the action and shows the new state
// (adding to or removing from collections, several in a row); otherwise it
// closes first.
// `hex`: a coloured dot before the label; `""` for an empty one, which keeps
// the label aligned with those of coloured items.
function menuButton(label, action, keepOpen = false, hex = null) {
  const button = document.createElement("button");
  if (hex !== null) button.append(dot(hex));
  button.append(label);
  button.addEventListener("click", async () => {
    if (!keepOpen) closeContextMenu();
    await action();
    if (keepOpen) updateContextMenu();
  });
  return button;
}

// Where the menu was opened, to draw it again at the same place.
let menuPosition = { x: 0, y: 0 };

// Draws the open menu again; closes it if the selection is now empty (its
// samples left the open collection). Left closed if it was closed meanwhile
// (a click elsewhere, Esc).
function updateContextMenu() {
  if (contextMenu.hidden) return;
  if (chosenPaths().length === 0) closeContextMenu();
  else openContextMenu(menuPosition.x, menuPosition.y);
}

// The collections that already hold the samples are shown, marked and
// greyed, but cannot be clicked.
// Each opening gets a number, and closing the menu moves it on: an answer
// of `collections_holding` coming after Esc, a click elsewhere or another
// right-click must not show the menu again.
let menuOpening = 0;

async function openContextMenu(x, y) {
  menuPosition = { x, y };
  const opening = ++menuOpening;
  let holding;
  try {
    holding = await invoke("collections_holding", { paths: chosenPaths() });
  } catch (error) {
    showError(error);
    return;
  }
  if (opening !== menuOpening) return;
  const items = [menuLabel("Add to collection")];
  for (const collection of collections) {
    if (holding.includes(collection.id)) {
      // Already there: the name cannot be clicked, a × next to it takes the
      // samples out.
      const already = menuButton(
        `✓ ${collection.name}`,
        () => {},
        false,
        colorHex(collection.color_id) ?? "",
      );
      already.disabled = true;
      already.title = "Already in this collection";
      const remove = menuButton(
        "×",
        () => removeFromCollection(undefined, collection.id),
        true,
      );
      remove.className = "remove-from";
      remove.title = `Remove from ${collection.name}`;
      const line = document.createElement("div");
      line.className = "held";
      line.append(already, remove);
      items.push(line);
    } else {
      items.push(
        menuButton(
          collection.name,
          () => addToCollection(collection),
          true,
          colorHex(collection.color_id) ?? "",
        ),
      );
    }
  }
  // A new collection, named here, gets the sample at once.
  const newName = document.createElement("input");
  newName.placeholder = "New collection…";
  newName.spellcheck = false;
  newName.addEventListener("keydown", async (event) => {
    // The keys typed here are for the name only: Enter must not also replay
    // the sample, nor the arrows browse the list.
    event.stopPropagation();
    if (event.key === "Escape") closeContextMenu();
    if (event.key !== "Enter" || newName.value.trim() === "") return;
    const name = newName.value.trim();
    const id = await createCollection(name);
    if (id !== null) await addToCollection({ id, name });
    updateContextMenu();
  });
  items.push(newName);
  if (currentCollection !== null) {
    items.push(document.createElement("hr"));
    items.push(menuButton("Remove from this collection (Del)", removeFromCollection));
  }
  items.push(document.createElement("hr"));
  items.push(detectButton());
  const automaticKey = menuButton("Automatic key", () => setChosen("set_key", "auto"));
  automaticKey.title = "Back to the key read in the name or the file";
  items.push(automaticKey);
  // The files themselves: a DAW project pointing to the old path will no
  // longer find them, hence the explicit wording.
  items.push(document.createElement("hr"));
  items.push(menuButton(isMac ? "Show in Finder" : "Show in file manager", revealChosen));
  if (chosenPaths().length === 1) items.push(renameButton());
  items.push(menuButton("Move file(s) to folder…", moveChosen));
  const convert = menuButton("Convert file(s)…", openConvert);
  if (!ffmpegFound) {
    convert.disabled = true;
    convert.title = "Converting needs ffmpeg, not found on this computer";
  }
  items.push(convert);
  items.push(menuButton("Move file(s) to the trash…", trashChosen));
  showMenu(items, x, y);
}

// "Detect BPM…": the button opens the methods in the menu; with a single
// sample, each shows what it would give, greyed when it gives nothing (the
// sound is only known once decoded).
const tempoMethods = [
  ["name", "From the name"],
  ["file", "From the file (ACID chunk)"],
  ["length", "From the length"],
  ["sound", "From the sound"],
];

function detectButton() {
  const button = document.createElement("button");
  button.textContent = "Detect BPM…";
  button.title = "Work out the tempo by a method of your choice, over any other, even one set by hand";
  button.addEventListener("click", async () => {
    const paths = chosenPaths();
    let values = null;
    if (paths.length === 1) {
      try {
        values = await invoke("tempo_methods", { path: paths[0] });
      } catch (error) {
        showError(error);
      }
    }
    const choices = tempoMethods.map(([method, label]) => {
      const value = values?.[method];
      const shown = values && method !== "sound" ? `${label}: ${value ?? "none"}` : label;
      const choice = menuButton(shown, () => detectChosen(method));
      choice.classList.add("method");
      if (values && method !== "sound" && value === null) choice.disabled = true;
      if (method === "sound") choice.title = "Decodes the file again";
      return choice;
    });
    const label = menuLabel("Detect BPM");
    const automatic = menuButton("Automatic", () => setChosen("set_tempo", "auto"));
    automatic.classList.add("method");
    automatic.title = "Back to the tempo worked out: name, file, then the sound for a loop";
    button.replaceWith(label, automatic, ...choices);
  });
  return button;
}

// Back to automatic for the selection: `command` is `set_tempo` or `set_key`.
async function setChosen(command, value) {
  const paths = chosenPaths();
  try {
    await invoke(command, { paths, value });
  } catch (error) {
    showError(error);
    return;
  }
  await refresh({ stopIfGone: false });
  flash(rowsOf(paths));
}

// The tempo found by `method` replaces any other, even one typed by hand.
// The sound is decoded again for each file, hence the status line.
async function detectChosen(method) {
  const paths = chosenPaths();
  status.textContent = `Detecting the tempo of ${paths.length} sample(s)…`;
  let result;
  try {
    result = await invoke("detect_tempo", { paths, method });
  } catch (error) {
    showError(error);
    return;
  }
  await refresh({ stopIfGone: false });
  flash(rowsOf(paths));
  status.textContent = `Tempo detected for ${result.detected} sample(s)`;
  if (result.failed > 0) {
    const why = {
      name: "no tempo in their name",
      file: "no ACID chunk",
      length: "their length gives no whole tempo",
      sound: "too short, longer than 64 s, or unreadable",
    }[method];
    showError(`No tempo found for ${result.failed} sample(s): ${why}`);
  }
}

// Shows the shared menu with `items` at (x, y), kept inside the window.
function showMenu(items, x, y) {
  contextMenu.replaceChildren(...items);
  contextMenu.hidden = false;
  placeInWindow(contextMenu, x, y);
}

function placeInWindow(element, x, y) {
  const { width, height } = element.getBoundingClientRect();
  element.style.left = `${Math.max(4, Math.min(x, window.innerWidth - width - 4))}px`;
  element.style.top = `${Math.max(4, Math.min(y, window.innerHeight - height - 4))}px`;
}

// A heading inside a menu.
function menuLabel(text, tag = "div") {
  const label = document.createElement(tag);
  label.className = "label";
  label.textContent = text;
  return label;
}

function closeContextMenu() {
  contextMenu.hidden = true;
  menuOpening += 1;
}

// "Rename file…": the button turns into a box holding the name without its
// extension (kept as it is). Enter renames, Esc closes the menu.
function renameButton() {
  const button = document.createElement("button");
  button.textContent = "Rename file…";
  button.title = "Renames the file on disk; a DAW project using it will no longer find it";
  button.addEventListener("click", () => {
    const path = chosenPaths()[0];
    const { name } = splitPath(path);
    const dot = name.lastIndexOf(".");
    const input = document.createElement("input");
    input.spellcheck = false;
    input.value = dot > 0 ? name.slice(0, dot) : name;
    input.title = dot > 0 ? `The extension ${name.slice(dot)} is kept` : "";
    input.addEventListener("keydown", async (event) => {
      // The keys typed here are for the name only (see the collection box).
      event.stopPropagation();
      if (event.key === "Escape") closeContextMenu();
      if (event.key !== "Enter" || input.value.trim() === "") return;
      try {
        const renamed = await invoke("rename_sample", { path, name: input.value });
        closeContextMenu();
        await pathsChanged(new Map([[path, renamed]]));
      } catch (error) {
        showError(error);
        input.select();
      }
    });
    button.replaceWith(input);
    input.focus();
    input.select();
  });
  return button;
}

// Moves the selected samples' files into a folder chosen in the system's
// folder chooser, inside a scanned folder.
async function moveChosen() {
  const paths = chosenPaths();
  const folder = await window.__TAURI__.dialog.open({
    directory: true,
    title: `Move ${paths.length} file(s) to…`,
    defaultPath: splitPath(paths[0]).folder,
  });
  if (folder === null) return;
  try {
    const moved = await invoke("move_samples", { paths, folder });
    await pathsChanged(new Map(paths.map((path, i) => [path, moved[i]])));
    status.textContent = `${moved.length} file(s) moved to ${folder}`;
  } catch (error) {
    // Some may have moved before the failure: the list shows where they are.
    await refresh({ stopIfGone: false });
    showError(error);
  }
}

// The selected samples' files, selected in the system's file manager.
async function revealChosen() {
  try {
    await invoke("reveal", { paths: chosenPaths() });
  } catch (error) {
    showError(error);
  }
}

// Converting samples, through ffmpeg (Rust side): the window asks the
// format and its settings, remembered for the next time.
const convertDialog = document.getElementById("convert");
const convertFields = {
  format: document.getElementById("convert-format"),
  bits: document.getElementById("convert-bits"),
  mp3Bitrate: document.getElementById("convert-mp3"),
  oggQuality: document.getElementById("convert-ogg"),
  mono: document.getElementById("convert-mono"),
};
let ffmpegFound = false;
invoke("ffmpeg_available").then((found) => {
  ffmpegFound = found;
});

let convertOptions = {
  format: "wav",
  bits: null,
  mp3Bitrate: 320,
  oggQuality: 8,
  mono: false,
  ...storedJson("convertOptions", {}),
};
// The paths to convert, kept while the window is open.
let convertPaths = [];

// Only the settings of the chosen format are shown.
function showConvertFields() {
  for (const row of convertDialog.querySelectorAll("[data-for]")) {
    row.hidden = !row.dataset.for.split(" ").includes(convertFields.format.value);
  }
}
convertFields.format.addEventListener("change", showConvertFields);

function openConvert() {
  convertPaths = chosenPaths();
  if (convertPaths.length === 0) return;
  convertFields.format.value = convertOptions.format;
  convertFields.bits.value = convertOptions.bits ?? "";
  convertFields.mp3Bitrate.value = String(convertOptions.mp3Bitrate);
  convertFields.oggQuality.value = String(convertOptions.oggQuality);
  convertFields.mono.checked = convertOptions.mono;
  // Offered only when there is something to mix down: a sample with more
  // than one channel (or whose channels are unknown). Hidden, it keeps the
  // remembered choice for the next stereo sample.
  const anyStereo = selectedSamples().some((sample) => sample.channels !== 1);
  document.getElementById("convert-mono-box").hidden = !anyStereo;
  const what = convertPaths.length === 1 ? splitPath(convertPaths[0]).name : `${convertPaths.length} files`;
  document.getElementById("convert-title").textContent = `Convert ${what}`;
  showConvertFields();
  convertDialog.returnValue = "";
  convertDialog.showModal();
  convertFields.format.focus();
}

// The window closes on any of its buttons (a `method="dialog"` form); only
// "Convert" goes on.
convertDialog.addEventListener("close", async () => {
  if (convertDialog.returnValue !== "convert") return;
  convertOptions = {
    format: convertFields.format.value,
    bits: convertFields.bits.value === "" ? null : Number(convertFields.bits.value),
    mp3Bitrate: Number(convertFields.mp3Bitrate.value),
    oggQuality: Number(convertFields.oggQuality.value),
    mono: convertFields.mono.checked,
  };
  store("convertOptions", JSON.stringify(convertOptions));
  const paths = convertPaths;
  status.textContent = `Converting ${paths.length} file(s)…`;
  let copies = [];
  try {
    copies = await invoke("convert_samples", { paths, options: convertOptions });
  } catch (error) {
    showError(error);
  }
  await refresh({ stopIfGone: false });
  // The copies joined their originals' collections: the counts change.
  await loadCollections();
  if (copies.length > 0) {
    status.textContent = `${copies.length} copy(ies) written, next to the original(s)`;
    flash(rowsOf(copies));
  }
});
closeOnPressOutside(convertDialog);

// Sends the selected samples' files to the system's trash, after asking,
// always: these are files, not links, and their tags and collections are
// lost, even if a file is put back. The Settings checkbox does not apply.
async function trashChosen() {
  const paths = chosenPaths();
  const what = paths.length === 1 ? splitPath(paths[0]).name : `${paths.length} files`;
  const confirmed = await window.__TAURI__.dialog.ask(
    `Move ${what} to the trash?\n\n` +
      "Their tags, collections and favorite are lost, even if put back from the trash.",
    { title: "Move to the trash", kind: "warning", okLabel: "Move to the trash", cancelLabel: "Cancel" },
  );
  if (!confirmed) return;
  try {
    const trashed = await invoke("trash_samples", { paths });
    status.textContent = `${trashed} file(s) moved to the trash`;
  } catch (error) {
    showError(error);
  }
  // Those trashed are gone from the list; a sound of theirs stops.
  const statusText = status.textContent;
  await refresh();
  await loadCollections();
  status.textContent = statusText;
}

// Files renamed or moved (old path → new path): the selection, the sound
// playing and the list follow them, nothing stops.
async function pathsChanged(renamed) {
  for (const sample of samples) {
    if (renamed.has(sample.path)) sample.path = renamed.get(sample.path);
  }
  selectedPaths = new Set([...selectedPaths].map((path) => renamed.get(path) ?? path));
  if (renamed.has(playingPath)) playingPath = renamed.get(playingPath);
  await refresh({ stopIfGone: false });
  flash(rowsOf([...renamed.values()]));
}

tbody.addEventListener("contextmenu", (event) => {
  const index = rowIndex(event);
  if (index < 0) return;
  event.preventDefault();
  // A row outside the selection becomes the only one selected; inside it,
  // the menu acts on the whole selection.
  if (!selectedPaths.has(samples[index].path)) select(index, "single", false);
  openContextMenu(event.clientX, event.clientY);
});

// Also while a row's menu is still waiting for its answer (still hidden): a
// press elsewhere means it is no longer wanted.
document.addEventListener("mousedown", (event) => {
  if (!contextMenu.contains(event.target)) closeContextMenu();
});
window.addEventListener("blur", closeContextMenu);
listBox.addEventListener("scroll", closeContextMenu);
