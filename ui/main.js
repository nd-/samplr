// `withGlobalTauri` in tauri.conf.json exposes the Tauri API as
// `window.__TAURI__`, so the page needs no build tool to import it.
const { invoke } = window.__TAURI__.core;

const searchBox = document.getElementById("search");
const tbody = document.getElementById("samples");
const status = document.getElementById("status");
const canvas = document.getElementById("wave");
const tagList = document.getElementById("tags");
const tagInput = document.getElementById("new-tags");
const stopButton = document.getElementById("stop");

let samples = [];
let selected = -1;
let peaks = [];

// Paths come from Mac, Linux or Windows: split on both separators.
function splitPath(path) {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return { folder: path.slice(0, cut), name: path.slice(cut + 1) };
}

function formatDuration(seconds) {
  if (seconds === null) return "?";
  if (seconds < 60) return `${seconds.toFixed(2)} s`;
  const s = Math.floor(seconds % 60);
  return `${Math.floor(seconds / 60)}:${String(s).padStart(2, "0")}`;
}

function cell(text, className) {
  const td = document.createElement("td");
  td.textContent = text;
  if (className) td.className = className;
  return td;
}

function render() {
  const rows = document.createDocumentFragment();
  samples.forEach((sample, index) => {
    const { folder, name } = splitPath(sample.path);
    const tr = document.createElement("tr");
    tr.title = sample.path;
    const star = cell(sample.favorite ? "★" : "☆", "star");
    star.title = "Favorite (Ctrl+D)";
    // Only toggles the favorite: the row is not selected, nothing plays.
    star.addEventListener("click", (event) => {
      event.stopPropagation();
      toggleFavorite(index);
    });
    tr.append(
      star,
      cell(name),
      cell(folder, "folder"),
      cell(formatDuration(sample.duration_secs), "number"),
      cell(sample.tags.join(" "), "tags"),
    );
    // In an open collection, a button at the end of the row takes that
    // sample out of it, whether it is selected or not.
    if (currentCollection !== null) {
      const remove = document.createElement("button");
      remove.textContent = "×";
      remove.title = "Remove from this collection";
      remove.addEventListener("click", (event) => {
        event.stopPropagation();
        removeFromCollection([sample.path]);
      });
      const end = cell("", "remove");
      end.append(remove);
      tr.append(end);
    }
    tr.addEventListener("click", () => select(index));
    tr.addEventListener("mousedown", (event) => watchForDrag(event, sample));
    rows.append(tr);
  });
  tbody.replaceChildren(rows);
  removeHeader.hidden = currentCollection === null;
  status.textContent = `${samples.length} sample(s)`;
}

const removeHeader = document.querySelector("th.remove");

// `andPlay`: false for a right-click, which selects without playing.
function select(index, andPlay = true) {
  if (index < 0 || index >= samples.length) return;
  tbody.children[selected]?.classList.remove("selected");
  selected = index;
  const row = tbody.children[selected];
  row.classList.add("selected");
  row.scrollIntoView({ block: "nearest" });
  if (andPlay) play();
  loadWaveform();
  renderTags();
}

// The tags of the selected sample, each with a button to remove it.
function renderTags() {
  const sample = samples[selected];
  tagInput.disabled = !sample;
  const chips = (sample?.tags ?? []).map((tag) => {
    const chip = document.createElement("span");
    chip.className = "tag";
    chip.textContent = tag;
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.title = `Remove ${tag}`;
    remove.addEventListener("click", () => removeTag(tag));
    chip.append(remove);
    return chip;
  });
  tagList.replaceChildren(...chips);
}

// Called with the sample's new tags after a change: the tag bar and the
// list row both show them.
function showTags(sample, tags) {
  sample.tags = tags;
  const index = samples.indexOf(sample);
  const row = tbody.children[index];
  if (row) row.querySelector(".tags").textContent = tags.join(" ");
  if (index === selected) renderTags();
}

async function addTags() {
  const sample = samples[selected];
  const tags = tagInput.value.split(/\s+/).filter((tag) => tag !== "");
  if (!sample || tags.length === 0) return;
  try {
    showTags(sample, await invoke("add_tags", { path: sample.path, tags }));
    tagInput.value = "";
  } catch (error) {
    status.textContent = `Error: ${error}`;
  }
}

async function removeTag(tag) {
  const sample = samples[selected];
  if (!sample) return;
  try {
    showTags(sample, await invoke("remove_tag", { path: sample.path, tag }));
  } catch (error) {
    status.textContent = `Error: ${error}`;
  }
}

async function loadWaveform() {
  const path = samples[selected].path;
  peaks = [];
  drawWaveform();
  try {
    const result = await invoke("waveform", { path });
    // While a long file was being decoded, the selection may have moved on:
    // a late answer must not replace the waveform of the new sample.
    if (samples[selected]?.path !== path) return;
    peaks = result;
  } catch (error) {
    status.textContent = `Error: ${error}`;
  }
  drawWaveform();
}

// One vertical bar per pixel column, from the lowest to the highest value of
// the sound over that column, as in every audio editor.
function drawWaveform() {
  // The canvas is drawn at the screen's real resolution (twice the CSS size
  // on a Retina or HiDPI screen), otherwise the drawing looks blurry.
  const scale = window.devicePixelRatio;
  const width = Math.round(canvas.clientWidth * scale);
  const height = Math.round(canvas.clientHeight * scale);
  canvas.width = width;
  canvas.height = height;

  const context = canvas.getContext("2d");
  context.clearRect(0, 0, width, height);
  if (peaks.length === 0) return;

  context.fillStyle = getComputedStyle(canvas).getPropertyValue("--wave");
  const middle = height / 2;
  for (let x = 0; x < width; x++) {
    // The peaks covered by this column: several when there are more peaks
    // than pixels, a single one, repeated, when there are fewer.
    const first = Math.floor((x * peaks.length) / width);
    const last = Math.max(first + 1, Math.floor(((x + 1) * peaks.length) / width));
    let low = 0;
    let high = 0;
    for (let i = first; i < last; i++) {
      low = Math.min(low, peaks[i][0]);
      high = Math.max(high, peaks[i][1]);
    }
    const top = middle - high * middle;
    const bottom = middle - low * middle;
    // At least one pixel high, so that silence shows as a line.
    context.fillRect(x, top, 1, Math.max(1, bottom - top));
  }
}

window.addEventListener("resize", drawWaveform);

// A click on the waveform plays the sample from that point, whether it was
// playing or not.
canvas.addEventListener("click", (event) => {
  const duration = samples[selected]?.duration_secs;
  if (!(duration > 0)) return;
  const fraction = event.offsetX / canvas.clientWidth;
  play(Math.min(Math.max(fraction, 0), 1) * duration);
});

// Each sound played gets a new number: the end of a sound already replaced
// by another one may still be reported, and must then be ignored.
let playId = 0;
// Loop mode: set by "Play loop", kept when another sample is selected,
// left only with "Play".
let looping = false;

// `start`: where to start, in seconds from the beginning of the sample.
// The path of the sample playing, or `null`.
let playingPath = null;

function play(start = 0) {
  const sample = samples[selected];
  if (!sample) return;
  playId += 1;
  playingPath = sample.path;
  startPlayhead(sample.duration_secs, start);
  status.textContent = `▶ ${splitPath(sample.path).name}`;
  stopButton.disabled = false;
  invoke("play", { path: sample.path, id: playId, looping, start }).catch((error) => {
    stopPlayhead();
    status.textContent = `Error: ${error}`;
  });
}

// The audio thread reports the position every 100 ms; in between, the line
// moves on the clock, so that it glides instead of jumping.
const playhead = document.getElementById("playhead");
let playheadDuration = null;
// The last position reported: seconds from the start, and when it came.
let lastPosition = null;
// A single animation loop at a time, however many sounds are started.
let animating = false;

function startPlayhead(duration, start) {
  playheadDuration = duration;
  lastPosition = { seconds: start, at: performance.now() };
  // Unknown length (some MP3s): no line, its place could not be computed.
  playhead.hidden = !(duration > 0);
  if (!animating) {
    animating = true;
    requestAnimationFrame(movePlayhead);
  }
}

function stopPlayhead() {
  lastPosition = null;
  playhead.hidden = true;
}

function movePlayhead() {
  if (lastPosition === null || playhead.hidden) {
    animating = false;
    return;
  }
  let seconds = lastPosition.seconds + (performance.now() - lastPosition.at) / 1000;
  seconds = looping
    ? seconds % playheadDuration
    : Math.min(seconds, playheadDuration);
  playhead.style.left = `${(seconds / playheadDuration) * 100}%`;
  requestAnimationFrame(movePlayhead);
}

window.__TAURI__.event.listen("playback-position", (event) => {
  if (event.payload.id !== playId || lastPosition === null) return;
  lastPosition = { seconds: event.payload.seconds, at: performance.now() };
});

function showStopped() {
  playingPath = null;
  stopPlayhead();
  stopButton.disabled = true;
  status.textContent = `${samples.length} sample(s)`;
}

function stop() {
  invoke("stop");
  showStopped();
}

window.__TAURI__.event.listen("playback-ended", (event) => {
  if (event.payload === playId) showStopped();
});

stopButton.addEventListener("click", stop);

const playButton = document.getElementById("play");
const loopButton = document.getElementById("play-loop");

function setLooping(on) {
  looping = on;
  loopButton.setAttribute("aria-pressed", String(on));
}

playButton.addEventListener("click", () => {
  setLooping(false);
  play();
});
loopButton.addEventListener("click", () => {
  setLooping(true);
  play();
});

// The slider's position is squared: the ear hears loudness roughly on a
// logarithmic scale, and a linear slider would do almost nothing in its top
// half. The position is remembered between sessions, on this computer only.
const volumeSlider = document.getElementById("volume");

function applyVolume() {
  const position = volumeSlider.value / 100;
  invoke("set_volume", { volume: position * position });
  try {
    localStorage.setItem("volume", volumeSlider.value);
  } catch {
    // Storage unavailable: the volume is simply not remembered.
  }
}

try {
  const saved = localStorage.getItem("volume");
  if (saved !== null) volumeSlider.value = saved;
} catch {
  // Storage unavailable: keep the default position.
}
volumeSlider.addEventListener("input", applyVolume);
applyVolume();

// The web view would otherwise start its own drag (of the text under the
// mouse) alongside the native one, and take its place.
document.addEventListener("dragstart", (event) => event.preventDefault());

// A drag starts once the mouse has moved a few pixels with the button held
// down, so that a plain click still selects and plays.
function watchForDrag(event, sample) {
  if (event.button !== 0) return;
  const startX = event.clientX;
  const startY = event.clientY;

  function onMove(move) {
    if (Math.hypot(move.clientX - startX, move.clientY - startY) < 5) return;
    stopWatching();
    startDrag(sample);
  }
  function stopWatching() {
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", stopWatching);
  }
  document.addEventListener("mousemove", onMove);
  document.addEventListener("mouseup", stopWatching);
}

// The web page alone cannot hand a file to another application: the drag
// goes through the system's native drag and drop (tauri-plugin-drag). The
// file is offered as a copy, so the receiving app never moves the original.
async function startDrag(sample) {
  try {
    await window.__TAURI__.drag.startDrag({
      item: [sample.path],
      icon: dragImage(sample),
      mode: "copy",
    });
  } catch (error) {
    status.textContent = `Error: ${error}`;
  }
}

// The image under the cursor while dragging: the sample's name.
function dragImage(sample) {
  const label = splitPath(sample.path).name;
  const image = document.createElement("canvas");
  const context = image.getContext("2d");
  context.font = "14px system-ui, sans-serif";
  image.width = Math.ceil(context.measureText(label).width) + 20;
  image.height = 28;
  // Resizing a canvas resets its settings: the font is set again.
  context.font = "14px system-ui, sans-serif";
  context.fillStyle = "#f5a524";
  context.fillRect(0, 0, image.width, image.height);
  context.fillStyle = "#1b1b1b";
  context.textBaseline = "middle";
  context.fillText(label, 10, image.height / 2);
  return image.toDataURL("image/png");
}

// The folder is chosen in the system's own window; scanning a big folder
// takes a while, during which the button stays disabled.
const addFolderButton = document.getElementById("add-folder");

// The folder being scanned, or `null`: progress events arriving after the
// end of the scan are ignored. Without a value, the bar shows an activity
// (the files are being counted); then it fills up.
let scanning = null;
const scanBar = document.getElementById("scan-bar");

window.__TAURI__.event.listen("scan-progress", (event) => {
  if (scanning === null) return;
  const { done, total } = event.payload;
  scanBar.max = total;
  scanBar.value = done;
  status.textContent = `Scanning ${scanning}… ${done} / ${total}`;
});

async function addFolder() {
  const folder = await window.__TAURI__.dialog.open({
    directory: true,
    title: "Add a folder of samples",
  });
  if (folder === null) return;
  addFolderButton.disabled = true;
  status.textContent = `Scanning ${folder}…`;
  scanning = folder;
  scanBar.removeAttribute("value");
  scanBar.hidden = false;
  try {
    const added = await invoke("add_folder", { folder });
    await search();
    status.textContent =
      `${added.samples} sample(s) saved, ${added.unreadable} unreadable.`;
  } catch (error) {
    status.textContent = `Error: ${error}`;
  } finally {
    scanning = null;
    scanBar.hidden = true;
    addFolderButton.disabled = false;
  }
}

addFolderButton.addEventListener("click", addFolder);

// The scanned folders, each with the number of samples it holds and a button
// to remove it from the library.
const foldersDialog = document.getElementById("folders");
const folderList = document.getElementById("folder-list");

async function showFolders() {
  let folders;
  try {
    folders = await invoke("folders");
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  const items = folders.map((folder) => {
    const item = document.createElement("li");
    const path = document.createElement("span");
    path.className = "path";
    path.textContent = folder.path;
    if (!folder.found) {
      const missing = document.createElement("span");
      missing.className = "missing";
      missing.textContent = " (not found)";
      path.append(missing);
    }
    const count = document.createElement("span");
    count.className = "count";
    count.textContent = `${folder.samples} sample(s)`;
    const remove = document.createElement("button");
    remove.textContent = "Remove";
    remove.addEventListener("click", () => removeFolder(folder));
    item.append(path, count, remove);
    return item;
  });
  if (items.length === 0) {
    const empty = document.createElement("li");
    empty.textContent = "No folder yet: add one with + Folder.";
    items.push(empty);
  }
  folderList.replaceChildren(...items);
  if (!foldersDialog.open) foldersDialog.showModal();
}

async function removeFolder(folder) {
  const confirmed = await window.__TAURI__.dialog.ask(
    `Remove ${folder.path} from the library?\n\n` +
      `Its ${folder.samples} sample(s) leave the list. Their tags are kept, ` +
      "and come back if the folder is added again. The audio files are not touched.",
    { title: "Remove folder", kind: "warning" },
  );
  if (!confirmed) return;
  try {
    const hidden = await invoke("remove_folder", { folder: folder.path });
    await refresh();
    await showFolders();
    status.textContent = `Folder removed, ${hidden} sample(s) hidden.`;
  } catch (error) {
    status.textContent = `Error: ${error}`;
  }
}

document.getElementById("show-folders").addEventListener("click", showFolders);

// The folders changed on disk (samples added, renamed or deleted): the same
// search again, keeping the selected sample and the sound playing.
// `stopIfGone: false` when the selected sample only left the open collection:
// it is still in the library, its sound keeps playing.
async function refresh({ stopIfGone = true } = {}) {
  const selectedPath = samples[selected]?.path;
  const statusText = status.textContent;
  try {
    samples = await invoke("search", {
      query: searchBox.value,
      favorites: favoritesOnly,
      collection: currentCollection,
    });
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  render();
  // While a sound plays, the status line keeps showing it.
  if (!stopButton.disabled) status.textContent = statusText;
  selected = samples.findIndex((sample) => sample.path === selectedPath);
  if (selected >= 0) {
    tbody.children[selected].classList.add("selected");
  } else {
    // The selected sample left the library (folder removed, file deleted or
    // renamed): its sound stops with it. A sound that is merely outside the
    // current search is not selected, and keeps playing.
    if (stopIfGone && selectedPath !== undefined && selectedPath === playingPath) stop();
    peaks = [];
    drawWaveform();
  }
  renderTags();
}

window.__TAURI__.event.listen("library-changed", () => refresh());

// "★ Favorites" on: the list shows only the favorites.
let favoritesOnly = false;
const favoritesButton = document.getElementById("favorites-only");

favoritesButton.addEventListener("click", () => {
  setFavoritesOnly(!favoritesOnly);
  search();
});

// With the filter on, a sample no longer a favorite stays in the list until
// the next search: the list does not jump, and its sound keeps playing.
async function toggleFavorite(index) {
  const sample = samples[index];
  if (!sample) return;
  const favorite = !sample.favorite;
  try {
    await invoke("set_favorite", { path: sample.path, favorite });
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  sample.favorite = favorite;
  const star = tbody.children[index]?.querySelector(".star");
  if (star) star.textContent = favorite ? "★" : "☆";
}

// Collections: samples gathered by hand. `currentCollection` is the one
// open in the list, `null` for the whole library.
const collectionList = document.getElementById("collection-list");
const collectionForm = document.getElementById("new-collection");
const collectionName = document.getElementById("collection-name");
let collections = [];
let currentCollection = null;

async function loadCollections() {
  try {
    collections = await invoke("collections");
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  // The open collection may have been deleted meanwhile.
  if (currentCollection !== null && !collections.some((c) => c.id === currentCollection)) {
    currentCollection = null;
    search();
  }
  renderCollections();
}

function collectionItem(label, count, active, open) {
  const item = document.createElement("li");
  if (active) item.classList.add("active");
  const name = document.createElement("span");
  name.className = "name";
  name.textContent = label;
  name.addEventListener("click", open);
  item.append(name);
  if (count !== null) {
    const number = document.createElement("span");
    number.className = "count";
    number.textContent = count;
    item.append(number);
  }
  return item;
}

function renderCollections() {
  const items = [
    collectionItem("All samples", null, currentCollection === null, () => openCollection(null)),
  ];
  for (const collection of collections) {
    const item = collectionItem(
      collection.name,
      collection.samples,
      collection.id === currentCollection,
      () => openCollection(collection.id),
    );
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.title = `Delete the collection ${collection.name} (its samples stay in the library)`;
    remove.addEventListener("click", () => deleteCollection(collection));
    item.append(remove);
    items.push(item);
  }
  collectionList.replaceChildren(...items);
}

function setFavoritesOnly(on) {
  favoritesOnly = on;
  favoritesButton.setAttribute("aria-pressed", String(on));
}

function openCollection(id) {
  currentCollection = id;
  renderCollections();
  search();
}

// Deleting a whole collection loses the gathering: it is confirmed, unlike
// taking a few samples out of it.
async function deleteCollection(collection) {
  const confirmed = await window.__TAURI__.dialog.ask(
    `Delete the collection ${collection.name}?\n\n` +
      `Its ${collection.samples} sample(s) stay in the library.`,
    { title: "Delete collection", kind: "warning" },
  );
  if (!confirmed) return;
  try {
    await invoke("delete_collection", { collection: collection.id });
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  await loadCollections();
}

// Returns the new collection's id, or `null` if it could not be created.
async function createCollection(name) {
  try {
    const id = await invoke("create_collection", { name });
    await loadCollections();
    return id;
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return null;
  }
}

collectionForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const name = collectionName.value.trim();
  if (name === "") return;
  if ((await createCollection(name)) !== null) collectionName.value = "";
});

// The samples the collection commands act on: the selected one, for now.
function chosenPaths() {
  const sample = samples[selected];
  return sample ? [sample.path] : [];
}

async function addToCollection(collection) {
  const paths = chosenPaths();
  if (paths.length === 0) return;
  try {
    const added = await invoke("add_to_collection", { collection: collection.id, paths });
    status.textContent = `${added} sample(s) added to ${collection.name}.`;
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  await loadCollections();
}

// Only the links go: the samples stay in the library. No confirmation, as
// nothing is lost that cannot be added back.
// `paths`: the samples to take out, by default the selected one.
async function removeFromCollection(paths = chosenPaths()) {
  if (currentCollection === null || paths.length === 0) return;
  try {
    const removed = await invoke("remove_from_collection", {
      collection: currentCollection,
      paths,
    });
    // The removed sample leaves the list, but its sound keeps playing.
    await refresh({ stopIfGone: false });
    await loadCollections();
    status.textContent = `${removed} sample(s) removed from the collection.`;
  } catch (error) {
    status.textContent = `Error: ${error}`;
  }
}

// Right-click on a row: the row is selected (without playing it) and a menu
// offers the collections.
const contextMenu = document.getElementById("context-menu");

function menuButton(label, action) {
  const button = document.createElement("button");
  button.textContent = label;
  button.addEventListener("click", () => {
    closeContextMenu();
    action();
  });
  return button;
}

// The collections that already hold the samples are shown, marked and
// greyed, but cannot be clicked.
async function openContextMenu(x, y) {
  let holding;
  try {
    holding = await invoke("collections_holding", { paths: chosenPaths() });
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  const label = document.createElement("div");
  label.className = "label";
  label.textContent = "Add to collection";
  const items = [label];
  for (const collection of collections) {
    if (holding.includes(collection.id)) {
      const already = menuButton(`✓ ${collection.name}`, () => {});
      already.disabled = true;
      already.title = "Already in this collection";
      items.push(already);
    } else {
      items.push(menuButton(collection.name, () => addToCollection(collection)));
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
    closeContextMenu();
    const id = await createCollection(name);
    if (id !== null) await addToCollection({ id, name });
  });
  items.push(newName);
  if (currentCollection !== null) {
    items.push(document.createElement("hr"));
    items.push(menuButton("Remove from this collection (Del)", removeFromCollection));
  }
  contextMenu.replaceChildren(...items);
  contextMenu.hidden = false;
  // Kept inside the window.
  const { width, height } = contextMenu.getBoundingClientRect();
  contextMenu.style.left = `${Math.min(x, window.innerWidth - width - 4)}px`;
  contextMenu.style.top = `${Math.min(y, window.innerHeight - height - 4)}px`;
}

function closeContextMenu() {
  contextMenu.hidden = true;
}

tbody.addEventListener("contextmenu", (event) => {
  const row = event.target.closest("tr");
  if (!row) return;
  event.preventDefault();
  const index = [...tbody.children].indexOf(row);
  if (index !== selected) select(index, false);
  openContextMenu(event.clientX, event.clientY);
});

document.addEventListener("mousedown", (event) => {
  if (!contextMenu.hidden && !contextMenu.contains(event.target)) closeContextMenu();
});
window.addEventListener("blur", closeContextMenu);
document.getElementById("list").addEventListener("scroll", closeContextMenu);

async function search() {
  try {
    samples = await invoke("search", {
      query: searchBox.value,
      favorites: favoritesOnly,
      collection: currentCollection,
    });
    selected = -1;
    peaks = [];
    drawWaveform();
    render();
    renderTags();
  } catch (error) {
    status.textContent = `Error: ${error}`;
  }
}

// Searching on every keystroke would query the database for each letter:
// wait until typing pauses for 150 ms.
let searchTimer;
searchBox.addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(search, 150);
});

// The keys work wherever the focus is, search box included, so that one
// can type a search and browse the results without touching the mouse.
document.addEventListener("keydown", (event) => {
  // The folders window has its own keys (Esc closes it, Tab moves between
  // its buttons): the list behind must not react.
  if (foldersDialog.open) return;
  // Typing a collection name: Enter creates it (the form's own behaviour),
  // and the list's keys must not react.
  if (event.target === collectionName) return;
  if (!contextMenu.hidden) {
    if (event.key === "Escape") {
      closeContextMenu();
      event.preventDefault();
    }
    return;
  }
  // Del (Backspace on a Mac keyboard) takes the selected sample out of the
  // open collection; not while typing, where the keys edit the text.
  if (
    (event.key === "Delete" || event.key === "Backspace") &&
    currentCollection !== null &&
    !(event.target instanceof HTMLInputElement)
  ) {
    removeFromCollection();
    event.preventDefault();
    return;
  }
  // In the tag box, Enter adds the tags typed, Backspace on an empty box
  // removes the last tag, and Esc goes back to the search box.
  if (event.target === tagInput) {
    if (event.key === "Enter") {
      addTags();
      event.preventDefault();
      return;
    }
    if (event.key === "Backspace" && tagInput.value === "") {
      const tags = samples[selected]?.tags ?? [];
      if (tags.length > 0) removeTag(tags[tags.length - 1]);
      event.preventDefault();
      return;
    }
    if (event.key === "Escape") {
      searchBox.focus();
      event.preventDefault();
      return;
    }
  }
  // Ctrl+D, or Cmd+D on Mac (the usual bookmark keys), to make the selected
  // sample a favorite or no longer.
  if ((event.ctrlKey || event.metaKey) && event.key === "d") {
    toggleFavorite(selected);
    event.preventDefault();
    return;
  }
  // Ctrl+T, or Cmd+T on Mac, to tag the selected sample.
  if ((event.ctrlKey || event.metaKey) && event.key === "t") {
    if (!tagInput.disabled) tagInput.focus();
    event.preventDefault();
    return;
  }
  switch (event.key) {
    case "ArrowDown":
      select(selected + 1);
      break;
    case "ArrowUp":
      select(selected - 1);
      break;
    case "Enter":
      play();
      break;
    case "Escape":
      stop();
      break;
    default:
      return;
  }
  event.preventDefault();
});

search();
loadCollections();
