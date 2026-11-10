// `withGlobalTauri` in tauri.conf.json exposes the Tauri API as
// `window.__TAURI__`, so the page needs no build tool to import it.
const { invoke } = window.__TAURI__.core;

// Shortcuts are written for Linux ("Ctrl+D") and shown with the Mac's own
// keys there (⌘ for Ctrl, ⇧ for Shift, ⌫ for Del): the code already listens
// to `metaKey` as well as `ctrlKey`.
const isMac = /Mac/.test(navigator.platform || navigator.userAgent);

function shortcut(text) {
  if (!isMac) return text;
  return text.replaceAll("Ctrl+", "⌘").replaceAll("Shift+", "⇧").replaceAll("Del", "⌫");
}

// The titles written in index.html.
for (const element of document.querySelectorAll('[title*="Ctrl+"]')) {
  element.title = shortcut(element.title);
}

// The page's choices kept on this computer. `localStorage` throws when the
// storage is unavailable: nothing is then remembered, and each choice keeps
// its default.
function stored(key) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

// `null` forgets the key.
function store(key, value) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, String(value));
  } catch {
    // Not remembered: the choice lasts until the app closes.
  }
}

// A value stored as JSON, or `fallback` when missing or unreadable.
function storedJson(key, fallback) {
  try {
    return JSON.parse(stored(key)) ?? fallback;
  } catch {
    return fallback;
  }
}

// Settings > Confirmations: ask before deleting a collection, removing a
// folder or deleting a colour (on unless turned off). Remembered on
// this computer, like the theme.
let askBeforeDeleting = stored("askBeforeDeleting") !== "false";

// Returns true when the action may go ahead: confirmed, or no question asked.
async function confirmAction(message, title) {
  if (!askBeforeDeleting) return true;
  return window.__TAURI__.dialog.ask(message, { title, kind: "warning" });
}

const searchBox = document.getElementById("search");
const tbody = document.getElementById("samples");
const status = document.getElementById("status");

// An error: a red box above the status line, closed by its × or after
// 10 seconds, so that a refused action is not missed. The status line keeps
// it too.
const errorBox = document.getElementById("error-box");
const errorText = document.getElementById("error-text");
let errorTimer = null;

function showError(error) {
  status.textContent = `Error: ${error}`;
  errorText.textContent = String(error);
  errorBox.hidden = false;
  clearTimeout(errorTimer);
  errorTimer = setTimeout(hideError, 10000);
}

function hideError() {
  errorBox.hidden = true;
  clearTimeout(errorTimer);
}

document.getElementById("error-close").addEventListener("click", hideError);
// While the pointer is on the box (selecting its text to copy it), it does
// not close by itself; the 10 seconds start again when the pointer leaves.
errorBox.addEventListener("mouseenter", () => clearTimeout(errorTimer));
errorBox.addEventListener("mouseleave", () => {
  if (errorBox.hidden) return;
  clearTimeout(errorTimer);
  errorTimer = setTimeout(hideError, 10000);
});
const canvas = document.getElementById("wave");
const tagBar = document.getElementById("tagbar");
const tagInput = document.getElementById("new-tags");
const stopButton = document.getElementById("stop");

let samples = [];
// The current sample: the one played, whose waveform and tags are shown.
let selected = -1;
// The selected samples, by path rather than by row, so that the selection
// survives a refresh of the list. The current one is among them, unless it
// was just taken out with Ctrl+click.
let selectedPaths = new Set();
// Where a Shift+click or Shift+arrow range starts.
let anchor = -1;
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

// The list is virtual: only the rows on screen, and `overscan` more above
// and below, are in the table, between two empty rows that give it its full
// height. Building and laying out thousands of rows took a few hundred
// milliseconds at each search; a screenful takes a few. Rows have a fixed
// height (CSS), measured on the first row shown.
const listBox = document.getElementById("list");
// The header is a table of its own above the list, outside its scrolling: a
// `position: sticky` header made WebKitGTK draw the text of the rows passing
// under it over its top. The rows' table gets a copy of its columns.
const listHead = document.getElementById("list-head");
const tables = [listHead.querySelector("table"), listBox.querySelector("table")];
tables[1].prepend(listHead.querySelector("colgroup").cloneNode(true));
const overscan = 10;
let rowHeight = 0;
// The samples whose rows are in the table: `firstDrawn` to `lastDrawn`.
let firstDrawn = 0;
let lastDrawn = -1;

function spacer(height) {
  const tr = document.createElement("tr");
  tr.className = "spacer";
  const td = document.createElement("td");
  // Exactly the columns shown, counted on the header: one more would make a
  // phantom column that takes a share of the free width from Tags.
  td.colSpan = document.querySelectorAll("thead th:not([hidden])").length;
  td.style.height = `${height}px`;
  tr.append(td);
  return tr;
}

function buildRow(index) {
  const sample = samples[index];
  const { folder, name } = splitPath(sample.path);
  const tr = document.createElement("tr");
  tr.dataset.index = index;
  tr.title = sample.path;
  if (selectedPaths.has(sample.path)) tr.classList.add("selected");
  if (index === selected && selectedPaths.size > 1) tr.classList.add("current");
  // One row in two slightly darker, after the sample's place in the list:
  // the table's own children alternate differently as the virtual list
  // scrolls (the spacer row comes first).
  if (index % 2 === 1) tr.classList.add("striped");
  // In the header's order; a hidden column gets no cell.
  const cells = {
    favorite: () => {
      const star = cell(sample.favorite ? "★" : "☆", "star");
      star.classList.toggle("off", !sample.favorite);
      star.title = shortcut("Favorite (Ctrl+D)");
      return star;
    },
    name: () => cell(name),
    length: () => {
      const td = cell(formatDuration(sample.duration_secs), "number");
      td.title = barsText(sample);
      return td;
    },
    bpm: () => bpmCell(sample),
    key: () => keyCell(sample),
    loop: () => loopCell(sample),
    type: () => typeCell(sample),
    lufs: () => loudnessCell(sample),
    channels: () => cell(sample.channels === null ? "" : String(sample.channels), "number"),
    codec: () => cell(sample.codec ?? "", "codec"),
    collections: () => collectionsCell(sample.collections),
    tags: () => tagsCell(sample.tags),
    folder: () => cell(folder, "folder"),
  };
  for (const [key, make] of Object.entries(cells)) {
    if (!hiddenColumns.has(key)) tr.append(make());
  }
  // In an open collection, a button at the end of the row takes that
  // sample out of it, whether it is selected or not.
  if (currentCollection !== null) {
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.title = "Remove from this collection";
    const end = cell("", "remove");
    end.append(remove);
    tr.append(end);
  }
  return tr;
}

// The range of samples on screen, without the overscan.
function visibleRange() {
  const height = rowHeight || 26;
  const first = Math.floor(listBox.scrollTop / height);
  const count = Math.ceil(listBox.clientHeight / height);
  return [first, Math.min(samples.length - 1, first + count)];
}

function drawRows() {
  const [first, last] = visibleRange();
  firstDrawn = Math.max(0, first - overscan);
  lastDrawn = Math.min(samples.length - 1, last + overscan);
  const height = rowHeight || 26;
  const rows = document.createDocumentFragment();
  rows.append(spacer(firstDrawn * height));
  for (let index = firstDrawn; index <= lastDrawn; index++) rows.append(buildRow(index));
  rows.append(spacer(Math.max(0, samples.length - 1 - lastDrawn) * height));
  tbody.replaceChildren(rows);
  // The first time, the real height of a row; drawn again if the guess was
  // wrong, so that the spacers are right.
  if (rowHeight === 0 && lastDrawn >= firstDrawn) {
    rowHeight = rowElement(firstDrawn).getBoundingClientRect().height;
    drawRows();
  }
}

// Draws again only when the rows on screen are no longer all in the table.
function drawRowsIfNeeded() {
  const [first, last] = visibleRange();
  if (first < firstDrawn || (last > lastDrawn && lastDrawn < samples.length - 1)) drawRows();
}

let scrollPending = false;
listBox.addEventListener("scroll", () => {
  // The header follows the list sideways.
  listHead.scrollLeft = listBox.scrollLeft;
  if (scrollPending) return;
  scrollPending = true;
  requestAnimationFrame(() => {
    scrollPending = false;
    drawRowsIfNeeded();
  });
});
window.addEventListener("resize", drawRowsIfNeeded);

// The row of a sample, or `null` if it is off screen (not in the table).
function rowElement(index) {
  if (index < firstDrawn || index > lastDrawn) return null;
  return tbody.children[index - firstDrawn + 1] ?? null;
}

// Scrolls just enough for the row to show.
function ensureVisible(index) {
  if (index < 0 || rowHeight === 0) return;
  const top = index * rowHeight;
  const bottom = top + rowHeight;
  if (top < listBox.scrollTop) listBox.scrollTop = top;
  else if (bottom > listBox.scrollTop + listBox.clientHeight) {
    listBox.scrollTop = bottom - listBox.clientHeight;
  }
  drawRowsIfNeeded();
}

function render() {
  applyColumns();
  drawRows();
  applyColumnWidths();
  status.textContent = `${samples.length} sample(s)`;
}

// The rows are built again on scrolling: their events are listened to once,
// on the table body.
function rowIndex(event) {
  const row = event.target.closest("tr[data-index]");
  return row ? Number(row.dataset.index) : -1;
}

tbody.addEventListener("click", (event) => {
  const index = rowIndex(event);
  // A tempo or a key being edited: clicking in its box selects nothing.
  if (index < 0 || event.target instanceof HTMLInputElement) return;
  // The star only toggles the favorite: the row is not selected, nothing
  // plays. The same for the × of a collection.
  if (event.target.closest("td.star")) {
    toggleFavorite(index);
    return;
  }
  if (event.target.closest("td.remove button")) {
    removeFromCollection([samples[index].path]);
    return;
  }
  // A tempo, a key, loop / one-shot or a type is edited with a double-click:
  // a click there plays only once no second click has come, so that editing
  // stays silent, and the second click of the double-click does nothing.
  // Elsewhere, quick clicks on a row still retrigger the sound, like a pad.
  clearTimeout(delayedPlay);
  const editable = event.target.closest("td.bpm, td.key, td.loop, td.type") !== null;
  if (editable && event.detail > 1) return;
  const andPlay = autoplayBox.checked && !editable;
  const modified = event.metaKey || event.ctrlKey || event.shiftKey;
  // On a row of a multiple selection, the first click of a double-click
  // must not reduce the selection to that row: the edit is for the whole
  // selection. It is reduced only once no second click has come.
  if (editable && !modified && selectedPaths.size > 1 && selectedPaths.has(samples[index].path)) {
    selected = index;
    paintSelection();
    delayedPlay = setTimeout(() => {
      select(index, "single", autoplayBox.checked);
    }, doubleClickDelay);
    return;
  }
  // Cmd on a Mac, Ctrl elsewhere (Ctrl+click is a right-click on a Mac).
  if (event.metaKey || event.ctrlKey) select(index, "toggle", andPlay);
  else if (event.shiftKey) select(index, "range", andPlay);
  else select(index, "single", andPlay);
  if (editable && autoplayBox.checked && selectedPaths.has(samples[index].path)) {
    delayedPlay = setTimeout(() => {
      if (selected === index) play();
    }, doubleClickDelay);
  }
  // Autoplay off: moving to another sample silences the one playing, so that
  // what is heard is never another sample than the one selected.
  if (!autoplayBox.checked && playingPath !== null && samples[selected]?.path !== playingPath) {
    stop();
  }
});

// GTK's default double-click time: a click on an editable cell waits this
// long before playing, in case a second click makes it an edit.
const doubleClickDelay = 400;
let delayedPlay = null;

tbody.addEventListener("mousedown", (event) => {
  const index = rowIndex(event);
  if (index >= 0 && !(event.target instanceof HTMLInputElement)) watchForDrag(event, samples[index]);
});

const removeHeader = document.querySelector("th.remove");
const sortHeaders = [...document.querySelectorAll("th[data-sort]")];
// Each table's <colgroup> with all its <col>s, to put back those shown.
const columnGroups = tables.map((table) => {
  const group = table.querySelector("colgroup");
  return [group, [...group.children]];
});

// Columns hidden through the header's right-click menu, remembered on this
// computer. Name is always shown.
const columnLabels = {
  favorite: "★ Favorite",
  name: "Name",
  length: "Length",
  bpm: "BPM",
  key: "Key",
  loop: "Loop",
  type: "Type",
  lufs: "LUFS",
  channels: "Ch (channels)",
  codec: "Codec",
  collections: "Collections",
  tags: "Tags",
  folder: "Folder",
};
const hiddenColumns = new Set(storedJson("hiddenColumns", []));
hiddenColumns.delete("name");

// The column taking the width left: Folder, or Name when Folder is hidden.
function restColumn() {
  return hiddenColumns.has("folder") ? "name" : "folder";
}

// Shows the columns not hidden, and the × one in a collection only. A
// hidden column's <col> is taken out rather than given no width: WebKit
// treats a zero-width <col> as an automatic one, which then shares the free
// width.
function applyColumns() {
  removeHeader.hidden = currentCollection === null;
  for (const header of sortHeaders) header.hidden = hiddenColumns.has(header.dataset.sort);
  for (const [group, columns] of columnGroups) {
    group.replaceChildren(
      ...columns.filter((column) =>
        column.classList.contains("col-remove")
          ? currentCollection !== null
          : !hiddenColumns.has(column.dataset.col),
      ),
    );
  }
  // No line after the last column shown (`:last-child` would be the hidden
  // × header).
  const shown = [...removeHeader.parentElement.children].filter((header) => !header.hidden);
  for (const header of shown) header.classList.toggle("last", header === shown.at(-1));
  // Name's handle has nothing to set while it takes the rest.
  document.querySelector('.resizer[data-col="name"]').hidden = restColumn() === "name";
}

function toggleColumn(key) {
  if (hiddenColumns.has(key)) hiddenColumns.delete(key);
  else hiddenColumns.add(key);
  hiddenColumnsChanged();
}

function hiddenColumnsChanged() {
  store("hiddenColumns", JSON.stringify([...hiddenColumns]));
  render();
}

// The header's right-click menu: every column with a ✓ when shown. Stays
// open, so that several columns can be switched in a row.
function openColumnMenu(x, y) {
  // Shares the menu with the rows: a row's menu still waiting for its
  // answer must not replace this one.
  menuOpening += 1;
  const items = [menuLabel("Columns")];
  const choose = (action) => () => {
    action();
    openColumnMenu(x, y);
  };
  for (const [key, text] of Object.entries(columnLabels)) {
    const button = document.createElement("button");
    const check = document.createElement("span");
    check.className = "tick";
    check.textContent = hiddenColumns.has(key) ? "" : "✓";
    button.append(check, text);
    if (key === "name") {
      button.disabled = true;
      button.title = "Always shown";
    } else {
      button.addEventListener("click", choose(() => toggleColumn(key)));
    }
    items.push(button);
  }
  items.push(document.createElement("hr"));
  const all = document.createElement("button");
  all.textContent = "Show all columns";
  all.disabled = hiddenColumns.size === 0;
  all.addEventListener(
    "click",
    choose(() => {
      hiddenColumns.clear();
      hiddenColumnsChanged();
    }),
  );
  items.push(all);
  showMenu(items, x, y);
}

listHead.addEventListener("contextmenu", (event) => {
  event.preventDefault();
  openColumnMenu(event.clientX, event.clientY);
});

// Column widths, changed by dragging the handle on a header's right edge;
// a double-click on it brings the default back. Remembered between
// sessions, on this computer only. Folder, the last column, takes the rest
// of the width.
const defaultWidths = {
  name: 280,
  length: 90,
  bpm: 70,
  key: 60,
  loop: 60,
  type: 120,
  lufs: 60,
  channels: 40,
  codec: 90,
  collections: 160,
  tags: 200,
};
const columnWidths = { ...defaultWidths, ...storedJson("columnWidths", {}) };
// Folder had a width of its own before it took the rest.
delete columnWidths.folder;

// The table fills the list's width, Folder taking the rest, at least
// `minFolderWidth` (Name, when Folder is hidden, at least its own width).
// When that does not fit, the table grows wider than the list, which then
// scrolls sideways. The width is set outright: WebKit ignores `min-width`
// on a fixed-layout table.
const minFolderWidth = 120;

function applyColumnWidths() {
  const rest = restColumn();
  let total = 0;
  for (const [name, width] of Object.entries(columnWidths)) {
    // A hidden column's <col> is out of the tables, set anyway for later.
    for (const column of columnGroups.flatMap(([, columns]) => columns)) {
      if (column.dataset.col === name) column.style.width = name === rest ? "" : `${width}px`;
    }
    if (name !== rest && !hiddenColumns.has(name)) total += width;
  }
  const star = document.querySelector("th.star").offsetWidth;
  const remove = removeHeader.hidden ? 0 : removeHeader.offsetWidth;
  const restWidth = rest === "name" ? columnWidths.name : minFolderWidth;
  const needed = star + remove + total + restWidth;
  for (const table of tables) table.style.width = `${Math.max(listBox.clientWidth, needed)}px`;
}

window.addEventListener("resize", applyColumnWidths);

function saveColumnWidths() {
  store("columnWidths", JSON.stringify(columnWidths));
}

// A handle dragged with the mouse: `begin()` is called when it is pressed
// and returns `move(dx, dy)`, called with the distance from the press, and
// `end()`, called when it is released.
function dragHandle(handle, begin) {
  handle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    const { move, end } = begin();
    // The pointer stays with the handle even when it leaves it.
    handle.setPointerCapture(event.pointerId);
    handle.classList.add("dragging");
    const resize = (moved) => move(moved.clientX - event.clientX, moved.clientY - event.clientY);
    handle.addEventListener("pointermove", resize);
    handle.addEventListener(
      "pointerup",
      () => {
        handle.removeEventListener("pointermove", resize);
        handle.classList.remove("dragging");
        end();
      },
      { once: true },
    );
  });
}

for (const handle of document.querySelectorAll("th .resizer")) {
  const name = handle.dataset.col;
  dragHandle(handle, () => {
    const startWidth = columnWidths[name];
    return {
      move: (dx) => {
        columnWidths[name] = Math.max(40, startWidth + dx);
        applyColumnWidths();
      },
      end: saveColumnWidths,
    };
  });
  handle.addEventListener("dblclick", () => {
    columnWidths[name] = defaultWidths[name];
    applyColumnWidths();
    saveColumnWidths();
  });
  // Resizing must not also sort the column.
  handle.addEventListener("click", (event) => event.stopPropagation());
}

applyColumnWidths();

// Sorting by a column header: a first click sorts, a second one reverses.
// Done in the page, without asking the database again; kept from one search
// to the next. With no column chosen, the list keeps the database's order
// (by path).
let sortKey = null;
let sortAscending = true;

// Names compared like a person would: "kick 2" before "kick 10", case
// ignored.
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

// `null`: no value (unknown length, no tag), always at the end of the list
// whichever the direction.
const sortValues = {
  favorite: (sample) => (sample.favorite ? 0 : 1),
  name: (sample) => splitPath(sample.path).name,
  folder: (sample) => splitPath(sample.path).folder,
  length: (sample) => sample.duration_secs,
  bpm: (sample) => sample.bpm,
  key: (sample) => sample.key,
  loop: (sample) => (sample.looped ? 1 : 0),
  type: (sample) => typeLabel(sample) || null,
  lufs: (sample) => sample.lufs,
  channels: (sample) => sample.channels,
  codec: (sample) => sample.codec,
  tags: (sample) => sample.tags.join(" ") || null,
  collections: (sample) => collectionNames(sample.collections).join(" ") || null,
};

function compareValues(x, y) {
  if (x === null || y === null) return 0;
  return typeof x === "number" ? x - y : collator.compare(x, y);
}

function sortSamples() {
  if (sortKey === null) return;
  const value = sortValues[sortKey];
  // Each sample's value worked out once, rather than at each of the many
  // comparisons: some (name, folder, type, collections) are not free.
  const keyed = samples.map((sample) => [value(sample), sample]);
  keyed.sort(([x, a], [y, b]) => {
    if ((x === null) !== (y === null)) return x === null ? 1 : -1;
    const order = compareValues(x, y);
    // Equal values: by path, so that the order stays the same every time.
    return (sortAscending ? order : -order) || collator.compare(a.path, b.path);
  });
  keyed.forEach(([, sample], i) => {
    samples[i] = sample;
  });
}

for (const header of sortHeaders) {
  header.addEventListener("click", () => {
    if (sortKey === header.dataset.sort) {
      sortAscending = !sortAscending;
    } else {
      sortKey = header.dataset.sort;
      sortAscending = true;
    }
    for (const other of sortHeaders) {
      other.classList.toggle("sorted-ascending", other === header && sortAscending);
      other.classList.toggle("sorted-descending", other === header && !sortAscending);
    }
    // The selection and the current sample follow their rows.
    const currentPath = samples[selected]?.path;
    sortSamples();
    selected = samples.findIndex((sample) => sample.path === currentPath);
    anchor = selected;
    const statusText = status.textContent;
    render();
    status.textContent = statusText;
    ensureVisible(selected);
  });
}

function paintSelection() {
  for (let index = firstDrawn; index <= lastDrawn; index++) {
    const row = rowElement(index);
    row.classList.toggle("selected", selectedPaths.has(samples[index].path));
    row.classList.toggle("current", index === selected && selectedPaths.size > 1);
  }
}

// Every sample of the list, without playing: the current row stays, or
// becomes the first one.
function selectAll() {
  if (samples.length === 0) return;
  selectedPaths = new Set(samples.map((sample) => sample.path));
  const hadCurrent = selected >= 0;
  if (!hadCurrent) selected = 0;
  anchor = selected;
  paintSelection();
  renderTags();
  if (!hadCurrent) loadWaveform();
}

// The selected samples, in the list's order.
function selectedSamples() {
  return samples.filter((sample) => selectedPaths.has(sample.path));
}

// `mode`: "single" selects only this row, "toggle" adds it to the selection
// or takes it out (Ctrl/Cmd+click), "range" selects from the anchor to it
// (Shift). The row becomes the current one, and plays unless `andPlay` is
// false (right-click) or it was just taken out.
function select(index, mode = "single", andPlay = true) {
  if (index < 0 || index >= samples.length) return;
  const path = samples[index].path;
  if (mode === "toggle") {
    if (selectedPaths.has(path)) {
      selectedPaths.delete(path);
      andPlay = false;
    } else {
      selectedPaths.add(path);
    }
    anchor = index;
  } else if (mode === "range" && anchor >= 0) {
    const [first, last] = [Math.min(anchor, index), Math.max(anchor, index)];
    selectedPaths = new Set(samples.slice(first, last + 1).map((sample) => sample.path));
  } else {
    selectedPaths = new Set([path]);
    anchor = index;
  }
  selected = index;
  ensureVisible(selected);
  paintSelection();
  if (andPlay) play();
  loadWaveform();
  renderTags();
}

// The tags all the given samples carry: those the tag bar shows.
function commonTags(chosen) {
  if (chosen.length === 0) return [];
  return chosen[0].tags.filter((tag) => chosen.every((sample) => sample.tags.includes(tag)));
}

function renderTags() {
  const chosen = selectedSamples();
  tagInput.disabled = chosen.length === 0;
  tagInput.placeholder = chosen.length > 1 ? `Add tags to ${chosen.length} samples` : "Add tags";
  const common = commonTags(chosen);
  const chips = common.map((tag) => {
    const chip = document.createElement("span");
    chip.className = "tag";
    chip.textContent = tag;
    paint(chip, tagColor(tag));
    chip.title = "Right-click to give it a colour";
    chip.addEventListener("contextmenu", (event) => openTagColorMenu(event, tag));
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.title = `Remove ${tag}`;
    remove.addEventListener("click", () => removeTag(tag));
    chip.append(remove);
    return chip;
  });
  // The chips and the input are children of the same box, so that one gap
  // separates them all alike. Only the chips are replaced: moving the input
  // would take the focus from it while one is typing tags.
  for (const old of tagBar.querySelectorAll(".tag")) old.remove();
  tagInput.before(...chips);
  renderInfo();
}

// The Info panel, on the right: the current sample's details, what the list
// knows (format, tempo, key, loop and where they come from) and what the
// file says of itself (`file_info`: size, date, its own metadata). Drawn
// again whenever the selection or the samples change, with `renderTags`;
// the file is read again only when the current sample is another one.
const infoColumn = document.getElementById("info");
const infoList = document.getElementById("info-list");
let infoFor = null;
let infoOfFile = null;

const tempoOrigins = {
  name: "from the name",
  file: "from the file",
  length: "estimated from the length",
  sound: "detected from the sound",
  manual: "set by hand",
};

// A tempo worked out rather than read: shown "≈ 100", greyed.
function tempoEstimated(sample) {
  return sample.bpm_source === "length" || sample.bpm_source === "sound";
}

// Where a sample's tempo comes from, with how it was found when estimated:
// "detected from the sound: 8 beats in 4.00 s".
function tempoOrigin(sample) {
  let origin = tempoOrigins[sample.bpm_source];
  if (sample.bpm_fixed) origin += ", chosen with Detect BPM";
  if (!tempoEstimated(sample) || !(sample.duration_secs > 0)) return origin;
  const beats = Math.round((sample.bpm * sample.duration_secs) / 60);
  return `${origin}: ${beats} beats in ${sample.duration_secs.toFixed(2)} s`;
}

async function renderInfo() {
  const sample = samples[selected];
  renderPlayer(sample);
  // Folded, the file is not read: it is when the panel opens again.
  if (infoColumn.classList.contains("collapsed")) {
    infoFor = null;
    return;
  }
  if (!sample) {
    infoFor = null;
    infoOfFile = null;
    const hint = document.createElement("p");
    hint.className = "hint";
    hint.textContent = "Select a sample to see its details.";
    infoList.replaceChildren(hint);
    return;
  }
  if (infoFor !== sample.path) {
    infoFor = sample.path;
    infoOfFile = null;
    drawInfo(sample);
    try {
      const found = await invoke("file_info", { path: sample.path });
      // The selection may have moved on meanwhile.
      if (infoFor !== sample.path) return;
      infoOfFile = found;
    } catch (error) {
      if (infoFor === sample.path) infoOfFile = { error: String(error) };
    }
  }
  drawInfo(samples[selected] ?? sample);
}

function drawInfo(sample) {
  const items = [];
  const add = (label, value, origin) => {
    if (value === null || value === undefined || value === "") return;
    const term = document.createElement("dt");
    term.textContent = label;
    const description = document.createElement("dd");
    description.textContent = value;
    if (origin) {
      const small = document.createElement("span");
      small.className = "origin";
      small.textContent = ` ${origin}`;
      description.append(small);
    }
    items.push(term, description);
  };
  const heading = (text) => {
    const title = document.createElement("h3");
    title.textContent = text;
    items.push(title);
  };
  const { folder, name } = splitPath(sample.path);
  add("Name", name);
  add("Folder", folder);
  add("Format", formatOf(sample));
  add(
    "Length",
    sample.duration_secs === null
      ? null
      : [formatDuration(sample.duration_secs), barsText(sample)].filter(Boolean).join(" · "),
  );
  if (infoOfFile && !infoOfFile.error) {
    add("Size", formatSize(infoOfFile.size));
    if (infoOfFile.modified !== null) {
      add("Modified", new Date(infoOfFile.modified * 1000).toLocaleString());
    }
  }
  add("Tempo", sample.bpm === null ? null : `${sample.bpm} BPM`, tempoOrigin(sample));
  add("Key", sample.key);
  add("Loudness", loudnessText(sample) || null);
  add(
    "Loop",
    sample.loop_source ? (sample.looped ? "Loop" : "One-shot") : null,
    loopOrigins[sample.loop_source],
  );
  add("Type", typeLabel(sample) || null, typeOrigins[sample.type_source]);
  if (sample.tags.length > 0) add("Tags", sample.tags.join(", "));
  if (selectedPaths.size > 1) add("Selection", `${selectedPaths.size} samples`);
  if (infoOfFile?.error) {
    heading("In the file");
    add("Error", infoOfFile.error);
  } else if (infoOfFile && infoOfFile.tags.length > 0) {
    heading("In the file");
    for (const [label, value] of infoOfFile.tags) add(label, value);
  }
  infoList.replaceChildren(...items);
}

// The playback bar: the selected sample's name, then its tempo, key, loop,
// length and loudness on one line.
const nowName = document.getElementById("now-name");
const nowDetails = document.getElementById("now-details");

function renderPlayer(sample) {
  if (!sample) {
    nowName.textContent = "";
    nowDetails.textContent = "";
    return;
  }
  nowName.textContent = splitPath(sample.path).name;
  nowName.title = sample.path;
  const details = [];
  // Pitched or played at another tempo: the tempo and key heard, the
  // file's own between brackets.
  const shape = shapeFor(sample);
  if (sample.bpm !== null) {
    const estimated = tempoEstimated(sample) ? "≈ " : "";
    const bpm = Math.round(sample.bpm * shape.speed * shape.tempo * 10) / 10;
    details.push(estimated + (bpm === sample.bpm ? `${bpm} BPM` : `${bpm} BPM (${sample.bpm})`));
  }
  if (sample.key) {
    const heard = transpose(sample.key, Math.round(heardSemitones(sample)));
    details.push(heard === sample.key ? heard : `${heard} (${sample.key})`);
  }
  if (sample.loop_source) details.push(sample.looped ? "Loop" : "One-shot");
  if (sample.duration_secs !== null) {
    const bars = barsText(sample);
    details.push(formatDuration(sample.duration_secs) + (bars ? ` (${bars})` : ""));
  }
  if (sample.lufs !== null) details.push(`${sample.lufs.toFixed(1)} LUFS`);
  nowDetails.textContent = details.join(" · ");
}

const notes = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

// "Am" up 2 semitones: "Bm". Keys are written in sharps.
function transpose(key, steps) {
  const match = /^([A-G]#?)(.*)$/.exec(key);
  if (!match || steps === 0) return key;
  const index = notes.indexOf(match[1]);
  return notes[(((index + steps) % 12) + 12) % 12] + match[2];
}

// A loop's length in bars of four beats, from its tempo: "4 bars", "1 bar",
// "2 beats" under a bar, "≈ 4.5 bars" when it is not a whole number (a tail
// of reverb, a tempo estimated wrong). Empty for a one-shot or without a
// tempo.
function barsText(sample) {
  if (!sample.looped || !sample.bpm || !(sample.duration_secs > 0)) return "";
  const beats = (sample.bpm * sample.duration_secs) / 60;
  if (beats < 3.9) {
    const whole = Math.round(beats);
    return Math.abs(beats - whole) < 0.1 && whole > 0
      ? `${whole} beat${whole === 1 ? "" : "s"}`
      : `≈ ${beats.toFixed(1)} beats`;
  }
  const bars = beats / 4;
  const whole = Math.round(bars);
  if (Math.abs(bars - whole) < 0.05) return `${whole} bar${whole === 1 ? "" : "s"}`;
  return `≈ ${bars.toFixed(1)} bars`;
}

// "WAV · pcm_s24le · 24 bit · 44.1 kHz · stereo".
function formatOf(sample) {
  const extension = sample.path.slice(sample.path.lastIndexOf(".") + 1).toUpperCase();
  const parts = [extension];
  if (sample.codec) parts.push(sample.codec);
  if (sample.bits) parts.push(`${sample.bits} bit`);
  if (sample.sample_rate) parts.push(`${sample.sample_rate / 1000} kHz`);
  if (sample.channels) {
    parts.push(sample.channels === 1 ? "mono" : sample.channels === 2 ? "stereo" : `${sample.channels} channels`);
  }
  return parts.join(" · ");
}

function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

const infoToggle = document.getElementById("toggle-info");

function setInfoFolded(folded) {
  infoColumn.classList.toggle("collapsed", folded);
  infoToggle.textContent = folded ? "‹" : "›";
  infoToggle.title = shortcut(folded ? "Show the info (Ctrl+I)" : "Hide the info (Ctrl+I)");
  // The waveform and the list take the width freed or taken.
  drawWaveform();
  applyColumnWidths();
  store("infoFolded", folded);
  if (!folded) renderInfo();
}

function toggleInfo() {
  setInfoFolded(!infoColumn.classList.contains("collapsed"));
}

infoToggle.addEventListener("click", toggleInfo);

// Called after a change with each sample's new tags (same order): the list
// rows and the tag bar show them.
function showTags(changed, allTags) {
  changed.forEach((sample, i) => {
    sample.tags = allTags[i];
  });
  // A tag added may be coloured: the rows are drawn again (a screenful).
  drawRows();
  renderTags();
  // A tag may have appeared, gone, or changed count.
  loadTagList();
}

// A short light on what just changed, so that one sees the action took.
function flash(elements) {
  const shown = elements.filter(Boolean);
  for (const element of shown) element.classList.remove("flash");
  // Reading a size makes the browser apply the removals now: the animation
  // then starts again even if it was still running. Once for all of them.
  void document.body.offsetWidth;
  for (const element of shown) {
    element.classList.add("flash");
    element.addEventListener("animationend", () => element.classList.remove("flash"), {
      once: true,
    });
  }
}

// The rows on screen showing these paths (the others are left out).
function rowsOf(paths) {
  const wanted = new Set(paths);
  const rows = [];
  for (let index = firstDrawn; index <= lastDrawn; index++) {
    if (wanted.has(samples[index].path)) rows.push(rowElement(index));
  }
  return rows;
}

async function addTags() {
  const chosen = selectedSamples();
  const tags = tagInput.value.split(/\s+/).filter((tag) => tag !== "");
  if (chosen.length === 0 || tags.length === 0) return;
  try {
    const paths = chosen.map((sample) => sample.path);
    showTags(chosen, await invoke("add_tags", { paths, tags }));
    tagInput.value = "";
    flash([...rowsOf(paths), tagBar]);
  } catch (error) {
    showError(error);
  }
}

async function removeTag(tag) {
  const chosen = selectedSamples();
  if (chosen.length === 0) return;
  try {
    const paths = chosen.map((sample) => sample.path);
    showTags(chosen, await invoke("remove_tag", { paths, tag }));
  } catch (error) {
    showError(error);
  }
}

async function loadWaveform() {
  const path = samples[selected]?.path;
  if (path === undefined) return;
  peaks = [];
  drawWaveform();
  try {
    const result = await invoke("waveform", { path });
    // While a long file was being decoded, the selection may have moved on:
    // a late answer must not replace the waveform of the new sample.
    if (samples[selected]?.path !== path) return;
    peaks = result;
  } catch (error) {
    showError(error);
  }
  drawWaveform();
}

// A colour variable of the theme as `#rrggbb`. The stylesheet may write it
// `light-dark(…)` (the System theme) or `color-mix(…)`, which a canvas does
// not read: it then drew in black. The browser resolves it on an element.
function cssColor(name) {
  const probe = document.createElement("span");
  probe.style.color = `var(${name})`;
  document.body.append(probe);
  const computed = getComputedStyle(probe).color;
  probe.remove();
  // "rgb(196, 122, 8)", or "color(srgb 0.77 0.48 0.03)" for a mixed colour.
  const scale = computed.startsWith("color(") ? 255 : 1;
  const channels = computed.match(/[\d.]+/g).slice(0, 3).map((c) => Math.round(Number(c) * scale));
  return `#${channels.map((c) => c.toString(16).padStart(2, "0")).join("")}`;
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

  context.fillStyle = cssColor("--wave");
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

// The path of the sample playing, or `null`.
let playingPath = null;
// The sound playing is paused (Space): it resumes where it was.
let paused = false;

// Space: a loop, or any sample played with Loop on, pauses and resumes; a
// one-shot played once plays again from the start, as a pad would; nothing
// playing, the selected sample plays.
function playOrPause() {
  const sample = samples[selected];
  if (playingPath === null || (sample && sample.path !== playingPath)) {
    play();
    return;
  }
  if (sample && !sample.looped && !loopBox.checked && !paused) {
    play();
    return;
  }
  paused = !paused;
  invoke("pause", { paused }).catch(showError);
  updatePlayButton();
  // The playhead stops where the sound is, and goes on from there.
  if (lastPosition !== null) {
    const now = performance.now();
    if (paused) {
      lastPosition = {
        seconds: lastPosition.seconds + ((now - lastPosition.at) / 1000) * playSpeed(),
        at: now,
      };
    } else {
      lastPosition = { ...lastPosition, at: now };
      if (!animating) {
        animating = true;
        requestAnimationFrame(movePlayhead);
      }
    }
  }
  const name = splitPath(playingPath).name;
  status.textContent = `${paused ? "⏸" : "▶"} ${name}`;
}

// `start`: where to start, in seconds from the beginning of the sample.
function play(start = 0) {
  const sample = samples[selected];
  if (!sample) return;
  playId += 1;
  playingPath = sample.path;
  playingSample = sample;
  previousSpeed = playSpeed();
  paused = false;
  updatePlayButton();
  startPlayhead(sample.duration_secs, start);
  status.textContent = `▶ ${splitPath(sample.path).name}`;
  stopButton.disabled = false;
  const shape = shapeFor(sample);
  invoke("play", { path: sample.path, id: playId, looping: loopBox.checked, start, ...shape }).catch((error) => {
    stopPlayhead();
    showError(error);
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
  if (paused) {
    // Still drawn where it stopped; nothing moves until Space again.
    animating = false;
    return;
  }
  let seconds = lastPosition.seconds + ((performance.now() - lastPosition.at) / 1000) * playSpeed();
  seconds = loopBox.checked
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
  paused = false;
  updatePlayButton();
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
playButton.addEventListener("click", () => {
  playOrPause();
  // Back to the list, so that the arrow keys and Space act there.
  playButton.blur();
});

// ⏸ while a sound plays, ▶ otherwise (stopped or paused).
function updatePlayButton() {
  const playing = playingPath !== null && !paused;
  playButton.textContent = playing ? "⏸" : "▶";
}

// "Play loop": every sample played loops until the box is unchecked.
// Remembered between sessions, like the volume.
const loopBox = document.getElementById("loop");
loopBox.checked = stored("loop") === "true";
loopBox.addEventListener("change", () => {
  store("loop", loopBox.checked);
  // A sound already playing cannot change mode: the selected one starts
  // again in the new mode; another one, no longer selected, stops.
  if (playingPath !== null) {
    if (playingPath === samples[selected]?.path) play();
    else stop();
  }
  loopBox.blur();
});

// The slider's position is squared: the ear hears loudness roughly on a
// logarithmic scale, and a linear slider would do almost nothing in its top
// half. The position is remembered between sessions, on this computer only.
const volumeSlider = document.getElementById("volume");

function applyVolume() {
  const position = volumeSlider.value / 100;
  invoke("set_volume", { volume: position * position });
  store("volume", volumeSlider.value);
}

const savedVolume = stored("volume");
if (savedVolume !== null) volumeSlider.value = savedVolume;
volumeSlider.addEventListener("input", applyVolume);
applyVolume();

// How the preview is played: Pitch and Fine transpose, BPM is the tempo the
// samples whose tempo is known are played at. Tape changes pitch and tempo
// together, as a varispeed (lighter, and sometimes the sound wanted);
// otherwise one changes without the other (Signalsmith Stretch, see
// `stretch.rs`). Only Tape is remembered: a library heard out of tune or out
// of tempo at the next launch would mislead.
const pitchSlider = document.getElementById("pitch");
const fineSlider = document.getElementById("fine");
const pitchValue = document.getElementById("pitch-value");
const fineValue = document.getElementById("fine-value");
const targetBox = document.getElementById("target-bpm");
const tapeBox = document.getElementById("tape");
// The sample whose sound is playing, for the playhead's speed.
let playingSample = null;

function semitones() {
  return Number(pitchSlider.value) + Number(fineSlider.value) / 100;
}

// The tempo factor that brings `sample` to the BPM asked; 1 without one.
function tempoRatio(sample) {
  const target = Number(targetBox.value);
  if (!(target > 0) || !sample?.bpm) return 1;
  return Math.min(4, Math.max(0.25, target / sample.bpm));
}

// What the audio thread is sent: `speed` changes both, as a tape; `tempo`
// and `semitones` one without the other.
function shapeFor(sample) {
  const ratio = tempoRatio(sample);
  if (tapeBox.checked) return { speed: ratio * 2 ** (semitones() / 12), tempo: 1, semitones: 0 };
  return { speed: 1, tempo: ratio, semitones: semitones() };
}

// How much faster than the file the sound goes: the playhead's speed.
function playSpeed() {
  const shape = shapeFor(playingSample ?? samples[selected]);
  return shape.speed * shape.tempo;
}

// The semitones heard: with Tape, the tempo's change moves the pitch too.
function heardSemitones(sample) {
  const shape = shapeFor(sample);
  return shape.semitones + 12 * Math.log2(shape.speed);
}

function signed(value) {
  return value > 0 ? `+${value}` : String(value);
}

function applyPlayback() {
  // The playhead goes on from where it is at the old speed.
  if (lastPosition !== null && !paused) {
    const now = performance.now();
    lastPosition = {
      seconds: lastPosition.seconds + ((now - lastPosition.at) / 1000) * previousSpeed,
      at: now,
    };
  }
  previousSpeed = playSpeed();
  pitchValue.textContent = `${signed(Number(pitchSlider.value))} st`;
  fineValue.textContent = `${signed(Number(fineSlider.value))} ct`;
  pitchValue.classList.toggle("shifted", pitchSlider.value !== "0");
  fineValue.classList.toggle("shifted", fineSlider.value !== "0");
  targetBox.classList.toggle("shifted", targetBox.value !== "");
  invoke("set_playback", shapeFor(playingSample ?? samples[selected])).catch(showError);
  renderPlayer(samples[selected]);
}

let previousSpeed = 1;
for (const slider of [pitchSlider, fineSlider]) {
  slider.addEventListener("input", applyPlayback);
  slider.addEventListener("dblclick", () => {
    slider.value = 0;
    applyPlayback();
  });
}
targetBox.addEventListener("input", applyPlayback);
targetBox.addEventListener("keydown", (event) => {
  // The list's keys (arrows, Space, L) must not react while typing a tempo.
  event.stopPropagation();
  if (event.key === "Enter" || event.key === "Escape") targetBox.blur();
});
tapeBox.checked = stored("tape") === "true";
tapeBox.addEventListener("change", () => {
  store("tape", tapeBox.checked);
  applyPlayback();
  tapeBox.blur();
});
applyPlayback();

// Autoplay off: a click on a row selects it without playing it, to tag it or
// drag it in silence. The arrow keys, Enter and Play still play.
const autoplayBox = document.getElementById("autoplay");
autoplayBox.checked = stored("autoplay") !== "false";
autoplayBox.addEventListener("change", () => {
  store("autoplay", autoplayBox.checked);
  // Back to the list, so that the arrow keys move in it again.
  autoplayBox.blur();
});

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
// files are offered as copies, so the receiving app never moves the
// originals. Dragging a selected row takes the whole selection along.
async function startDrag(sample) {
  const paths = pathsFor(sample);
  const label = paths.length > 1 ? `${paths.length} samples` : splitPath(sample.path).name;
  try {
    await window.__TAURI__.drag.startDrag({
      item: paths,
      icon: dragImage(label),
      mode: "copy",
    });
  } catch (error) {
    showError(error);
  }
}

// The image under the cursor while dragging: the sample's name, or how
// many samples go.
function dragImage(label) {
  const image = document.createElement("canvas");
  const context = image.getContext("2d");
  context.font = "14px system-ui, sans-serif";
  image.width = Math.ceil(context.measureText(label).width) + 20;
  image.height = 28;
  // Resizing a canvas resets its settings: the font is set again.
  context.font = "14px system-ui, sans-serif";
  // In the theme's accent colour, the text black or white to read on it.
  const accent = cssColor("--wave");
  context.fillStyle = accent;
  context.fillRect(0, 0, image.width, image.height);
  context.fillStyle = textOn(accent);
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
  if (folder !== null) await scanFolder(folder);
}

async function scanFolder(folder) {
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
    // The folder may be saved but not followed (too many folders for the
    // system, no permission).
    await showLibraryProblems();
  } catch (error) {
    showError(error);
  } finally {
    scanning = null;
    scanBar.hidden = true;
    addFolderButton.disabled = false;
  }
}

addFolderButton.addEventListener("click", addFolder);

// Once, at the first launch where one is there: the Splice folders found and
// not followed yet are offered. The answer, yes or no, is remembered.
async function offerSpliceFolders() {
  try {
    if (localStorage.getItem("spliceOffered") === "true") return;
  } catch {
    return;
  }
  let found;
  try {
    found = await invoke("splice_folders");
  } catch (error) {
    showError(error);
    return;
  }
  if (found.length === 0) return;
  store("spliceOffered", true);
  for (const folder of found) {
    const add = await window.__TAURI__.dialog.ask(
      `samplr found the folder where Splice keeps your sounds:\n${folder}\n\nAdd it to the library? Its samples stay where they are. The tempo, key and tags Splice shows stay in the Splice app: samplr works them out from the names.`,
      { title: "Splice folder", kind: "info", okLabel: "Add", cancelLabel: "Not now" },
    );
    if (add) await scanFolder(folder);
  }
}

// The scanned folders, each with the number of samples it holds and a button
// to remove it from the library.
const foldersDialog = document.getElementById("folders");
const folderList = document.getElementById("folder-list");

async function showFolders() {
  let folders;
  try {
    folders = await invoke("folders");
  } catch (error) {
    showError(error);
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
  const confirmed = await confirmAction(
    `Remove ${folder.path} from the library?\n\n` +
      `Its ${folder.samples} sample(s) leave the list. Their tags are kept, ` +
      "and come back if the folder is added again. The audio files are not touched.",
    "Remove folder",
  );
  if (!confirmed) return;
  try {
    const hidden = await invoke("remove_folder", { folder: folder.path });
    await refresh();
    await showFolders();
    status.textContent = `Folder removed, ${hidden} sample(s) hidden.`;
    await showLibraryProblems();
  } catch (error) {
    showError(error);
  }
}

document.getElementById("show-folders").addEventListener("click", showFolders);

// The folders changed on disk (samples added, renamed or deleted): the same
// search again, keeping the selected sample and the sound playing.
// `stopIfGone: false` when the selected sample only left the open collection:
// it is still in the library, its sound keeps playing.
async function refresh({ stopIfGone = true } = {}) {
  refreshWaiting = false;
  const selectedPath = samples[selected]?.path;
  const statusText = status.textContent;
  try {
    const found = await fetchSamples();
    if (found === null) return;
    samples = found;
    sortSamples();
  } catch (error) {
    showError(error);
    return;
  }
  // The selected samples still in the list stay selected.
  const present = new Set(samples.map((sample) => sample.path));
  selectedPaths = new Set([...selectedPaths].filter((path) => present.has(path)));
  selected = samples.findIndex((sample) => sample.path === selectedPath);
  anchor = selected;
  render();
  // While a sound plays, the status line keeps showing it.
  if (!stopButton.disabled) status.textContent = statusText;
  if (selected < 0) {
    // The selected sample left the library (folder removed, file deleted or
    // renamed): its sound stops with it. A sound that is merely outside the
    // current search is not selected, and keeps playing.
    if (stopIfGone && selectedPath !== undefined && selectedPath === playingPath) stop();
    peaks = [];
    drawWaveform();
  }
  renderTags();
  // Changes on disk, a folder removed, files trashed: the counts change.
  loadTagList();
}

// A change on disk, or the background pass telling its progress (every 5 s
// while it measures), draws the rows again: a tempo or key being edited in
// its cell, or a type being chosen, would be lost. The refresh then waits
// for the edit to end.
let refreshWaiting = false;

function editing() {
  return tbody.querySelector("input") !== null || picker !== null;
}

function refreshIfWaiting() {
  if (refreshWaiting && !editing()) refresh();
}

window.__TAURI__.event.listen("library-changed", () => {
  if (editing()) refreshWaiting = true;
  else refresh();
});

// Following the folders goes on when one of them fails (cannot be watched,
// a change not applied): the problems are kept on the Rust side and shown
// here. Asked at startup too, for those met before the page was listening.
async function showLibraryProblems() {
  let problems = [];
  try {
    problems = await invoke("library_problems");
  } catch (error) {
    showError(error);
    return;
  }
  if (problems.length > 0) showError(problems.join("\n"));
}

window.__TAURI__.event.listen("library-problem", showLibraryProblems);
showLibraryProblems();

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
  if (sample) await setFavorite([sample], !sample.favorite);
}

// Ctrl+D on a selection: all become favorites, unless they all are already.
async function toggleSelectedFavorites() {
  const chosen = selectedSamples();
  if (chosen.length === 0) return;
  await setFavorite(chosen, !chosen.every((sample) => sample.favorite));
}

async function setFavorite(chosen, favorite) {
  try {
    await invoke("set_favorite", { paths: chosen.map((sample) => sample.path), favorite });
  } catch (error) {
    showError(error);
    return;
  }
  const indexes = new Map(samples.map((sample, index) => [sample, index]));
  for (const sample of chosen) {
    sample.favorite = favorite;
    const star = rowElement(indexes.get(sample) ?? -1)?.querySelector(".star");
    if (star) {
      star.textContent = favorite ? "★" : "☆";
      star.classList.toggle("off", !favorite);
    }
  }
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
    showError(error);
    return;
  }
  // The open collection may have been deleted meanwhile.
  if (currentCollection !== null && !collections.some((c) => c.id === currentCollection)) {
    currentCollection = null;
    search();
  }
  renderCollections();
  // Names and colours of the Collections column.
  drawRows();
}

// The Tags box, under the collections: every tag with its number of
// samples. A click puts `#tag` in the search box, or takes it out, so that
// tags can be combined; those in the search are marked.
const tagList = document.getElementById("tag-list");
let tagCounts = [];

async function loadTagList() {
  try {
    tagCounts = await invoke("tags");
  } catch (error) {
    showError(error);
    return;
  }
  renderTagList();
}

// The `#tag` words of the search box, lowercase.
function searchedTags() {
  return searchBox.value
    .split(/\s+/)
    .filter((word) => word.startsWith("#") && word.length > 1)
    .map((word) => word.slice(1).toLowerCase());
}

// The Tags box's order: "name", or "count" (most used first, then by name).
// Remembered on this computer.
let tagSortOrder = stored("tagSort") === "count" ? "count" : "name";
const tagSortButtons = document.querySelectorAll("#tag-sort button");

for (const button of tagSortButtons) {
  button.addEventListener("click", () => {
    tagSortOrder = button.dataset.sort;
    store("tagSort", tagSortOrder);
    renderTagList();
  });
}

function renderTagList() {
  for (const button of tagSortButtons) {
    button.setAttribute("aria-pressed", String(button.dataset.sort === tagSortOrder));
  }
  const searched = searchedTags();
  // Names in the list's natural order ("kick 2" before "kick 10").
  const byName = (a, b) => collator.compare(a[0], b[0]);
  const sorted = [...tagCounts].sort(
    tagSortOrder === "count" ? (a, b) => b[1] - a[1] || byName(a, b) : byName,
  );
  const items = sorted.map(([name, count]) => {
    const item = collectionItem(
      name,
      count,
      searched.includes(name),
      () => toggleSearchedTag(name),
      undefined,
      tagColor(name) ?? "",
    );
    item.addEventListener("contextmenu", (event) => openTagColorMenu(event, name));
    return item;
  });
  if (items.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty";
    empty.textContent = "No tag yet";
    items.push(empty);
  }
  tagList.replaceChildren(...items);
}

function toggleSearchedTag(name) {
  const words = searchBox.value.split(/\s+/).filter((word) => word !== "");
  const kept = words.filter((word) => word.toLowerCase() !== `#${name}`);
  if (kept.length === words.length) kept.push(`#${name}`);
  searchBox.value = kept.join(" ");
  search();
}

// `id`: the collection's, kept on the item to find it under a drop; none for
// "All samples", which takes no drop.
function collectionItem(label, count, active, open, id, hex = null) {
  const item = document.createElement("li");
  if (active) item.classList.add("active");
  if (id !== undefined) item.dataset.id = id;
  const name = document.createElement("span");
  name.className = "name";
  // The whole name, when the column is too narrow for it.
  name.title = label;
  // An empty dot when `hex` is "": the names stay aligned with those of
  // coloured items. None for `null` ("All samples").
  if (hex !== null) name.append(dot(hex));
  name.append(label);
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
      collection.id,
      colorHex(collection.color_id) ?? "",
    );
    item.title = "Right-click to give it a colour";
    item.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      openColorMenu(event.clientX, event.clientY, collection.color_id, async (color) => {
        await invoke("set_collection_color", { collection: collection.id, color });
        await loadCollections();
      });
    });
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

// Deleting a whole collection loses the gathering: it is confirmed (unless
// turned off in Settings), unlike taking a few samples out of it.
async function deleteCollection(collection) {
  const confirmed = await confirmAction(
    `Delete the collection ${collection.name}?\n\n` +
      `Its ${collection.samples} sample(s) stay in the library.`,
    "Delete collection",
  );
  if (!confirmed) return;
  try {
    await invoke("delete_collection", { collection: collection.id });
  } catch (error) {
    showError(error);
    return;
  }
  await loadCollections();
  await refresh({ stopIfGone: false });
}

// Returns the new collection's id, or `null` if it could not be created.
async function createCollection(name) {
  try {
    const id = await invoke("create_collection", { name });
    await loadCollections();
    return id;
  } catch (error) {
    showError(error);
    return null;
  }
}

collectionForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const name = collectionName.value.trim();
  if (name === "") return;
  if ((await createCollection(name)) !== null) collectionName.value = "";
});

// The samples the collection commands act on: the selection.
function chosenPaths() {
  return selectedSamples().map((sample) => sample.path);
}

// What an action on a row acts on: the whole selection when the row is in
// it, otherwise that row alone.
function samplesFor(sample) {
  return selectedPaths.has(sample.path) ? selectedSamples() : [sample];
}

function pathsFor(sample) {
  return samplesFor(sample).map((chosen) => chosen.path);
}

// `paths`: by default the selection; the dropped files for a drop.
async function addToCollection(collection, paths = chosenPaths()) {
  if (paths.length === 0) return;
  let result;
  try {
    result = await invoke("add_to_collection", { collection: collection.id, paths });
  } catch (error) {
    showError(error);
    return;
  }
  // The Collections column changes, and dropped into the open collection,
  // the new samples show in the list.
  await loadCollections();
  await refresh({ stopIfGone: false });
  if (result.added > 0) {
    flash([...rowsOf(paths), collectionList.querySelector(`li[data-id="${collection.id}"]`)]);
  }
  const unknown = result.unknown > 0 ? ` (${result.unknown} not in the library, left out)` : "";
  status.textContent = `${result.added} sample(s) added to ${collection.name}${unknown}.`;
}

// Only the links go: the samples stay in the library. No confirmation, as
// nothing is lost that cannot be added back.
// `paths`: the samples to take out, by default the selected one.
// `collection`: by default the open one.
async function removeFromCollection(paths = chosenPaths(), collection = currentCollection) {
  if (collection === null || paths.length === 0) return;
  try {
    const removed = await invoke("remove_from_collection", { collection, paths });
    // The Collections column changes; from the open collection, the removed
    // sample leaves the list, but its sound keeps playing.
    await loadCollections();
    await refresh({ stopIfGone: false });
    status.textContent = `${removed} sample(s) removed from the collection.`;
  } catch (error) {
    showError(error);
  }
}

// The collections column can be folded into a thin strip (button or Ctrl+B,
// Cmd+B on Mac); remembered between sessions, on this computer only.
const collectionsColumn = document.getElementById("collections");
const collectionsToggle = document.getElementById("toggle-collections");

function setCollectionsFolded(folded) {
  collectionsColumn.classList.toggle("collapsed", folded);
  collectionsToggle.textContent = folded ? "›" : "‹";
  collectionsToggle.title = shortcut(
    folded ? "Show the collections (Ctrl+B)" : "Hide the collections (Ctrl+B)",
  );
  // The waveform and the list take the width freed or taken.
  drawWaveform();
  applyColumnWidths();
  store("collectionsFolded", folded);
}

// The collections column's width, changed by dragging its right edge, so
// that long names can be read; a double-click brings the default back.
// Remembered between sessions, on this computer only.
const collectionsResizer = document.getElementById("collections-resizer");

function setCollectionsWidth(width) {
  if (width === null) collectionsColumn.style.removeProperty("--collections-width");
  else {
    const widest = Math.max(160, window.innerWidth / 2);
    width = Math.round(Math.min(Math.max(width, 120), widest));
    collectionsColumn.style.setProperty("--collections-width", `${width}px`);
  }
  // The waveform and the list take the width freed or taken.
  drawWaveform();
  applyColumnWidths();
}

function saveCollectionsWidth(width) {
  store("collectionsWidth", width);
}

// The height shared between the collections and the tags, changed by
// dragging the handle between them: the collections' list gets `height`
// pixels, the tags the rest; `null` shares it equally. Remembered.
const listsResizer = document.getElementById("lists-resizer");
// Neither list is ever squeezed below this.
const minListHeight = 48;

function setCollectionsHeight(height) {
  if (height === null) {
    collectionList.style.removeProperty("flex");
    return;
  }
  const shared =
    collectionList.getBoundingClientRect().height + tagList.getBoundingClientRect().height;
  const clamped = Math.round(
    Math.min(Math.max(height, minListHeight), Math.max(minListHeight, shared - minListHeight)),
  );
  // May still shrink: a smaller window takes the room from both lists.
  collectionList.style.flex = `0 1 ${clamped}px`;
}

function saveCollectionsHeight(height) {
  store("collectionsHeight", height === null ? null : Math.round(height));
}

dragHandle(listsResizer, () => {
  const startHeight = collectionList.getBoundingClientRect().height;
  return {
    move: (dx, dy) => setCollectionsHeight(startHeight + dy),
    end: () => saveCollectionsHeight(collectionList.getBoundingClientRect().height),
  };
});
listsResizer.addEventListener("dblclick", () => {
  setCollectionsHeight(null);
  saveCollectionsHeight(null);
});

const savedHeight = stored("collectionsHeight");
if (savedHeight !== null) setCollectionsHeight(Number(savedHeight));

dragHandle(collectionsResizer, () => {
  const startWidth = collectionsColumn.getBoundingClientRect().width;
  return {
    move: (dx) => setCollectionsWidth(startWidth + dx),
    end: () => saveCollectionsWidth(collectionsColumn.getBoundingClientRect().width),
  };
});
collectionsResizer.addEventListener("dblclick", () => {
  setCollectionsWidth(null);
  saveCollectionsWidth(null);
});

const savedWidth = stored("collectionsWidth");
if (savedWidth !== null) setCollectionsWidth(Number(savedWidth));

function toggleCollections() {
  setCollectionsFolded(!collectionsColumn.classList.contains("collapsed"));
}

collectionsToggle.addEventListener("click", toggleCollections);

if (stored("collectionsFolded") === "true") setCollectionsFolded(true);

// Rows dragged out of the list, or files from a file manager, dropped on a
// collection of the left column are added to it. The drag goes through the
// system (see `startDrag`): the page only learns of it through Tauri's
// drag-and-drop events, which give the cursor's position in physical pixels.
let dropTarget = null;

function collectionUnder(position) {
  const scale = window.devicePixelRatio;
  const element = document.elementFromPoint(position.x / scale, position.y / scale);
  return element?.closest("#collection-list li[data-id]") ?? null;
}

function setDropTarget(item) {
  dropTarget?.classList.remove("drop-target");
  dropTarget = item;
  dropTarget?.classList.add("drop-target");
}

window.__TAURI__.webview.getCurrentWebview().onDragDropEvent((event) => {
  const { type } = event.payload;
  if (type === "leave") {
    setDropTarget(null);
    return;
  }
  const item = collectionUnder(event.payload.position);
  if (type !== "drop") {
    setDropTarget(item);
    return;
  }
  setDropTarget(null);
  const collection = collections.find((c) => String(c.id) === item?.dataset.id);
  if (collection) addToCollection(collection, event.payload.paths);
});

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

// The samples for the search box, the Favorites filter and the open
// collection as they are now; `null` when a newer request was made while
// this one ran (typing on, a change on disk): its answer is out of date and
// must not replace the list, whichever answer comes last.
let searchRequest = 0;

async function fetchSamples() {
  const request = ++searchRequest;
  const found = await invoke("search", {
    query: searchBox.value,
    favorites: favoritesOnly,
    collection: currentCollection,
  });
  return request === searchRequest ? found : null;
}

async function search() {
  updateTypeFilterButton();
  try {
    const found = await fetchSamples();
    if (found === null) return;
    samples = found;
    sortSamples();
    selected = -1;
    selectedPaths = new Set();
    anchor = -1;
    // New results: back to the top of the list.
    listBox.scrollTop = 0;
    peaks = [];
    drawWaveform();
    render();
    renderTags();
    // The `#tag` words typed or clicked are marked in the Tags box.
    renderTagList();
  } catch (error) {
    showError(error);
  }
}

// Searching on every keystroke would query the database for each letter:
// wait until typing pauses for 150 ms.
let searchTimer;
searchBox.addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(search, 150);
});

// A box where keys type text: search, tags, a tempo… not a slider or a
// checkbox.
function isTextEntry(element) {
  return (
    element instanceof HTMLTextAreaElement ||
    (element instanceof HTMLInputElement &&
      ["text", "search", "number"].includes(element.type))
  );
}

// The keys work wherever the focus is, search box included, so that one
// can type a search and browse the results without touching the mouse.
document.addEventListener("keydown", (event) => {
  // The folders window has its own keys (Esc closes it, Tab moves between
  // its buttons): the list behind must not react.
  if (foldersDialog.open || settingsDialog.open || convertDialog.open || picker) return;
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
      // The last chip of the bar: a tag of the whole selection, not one of
      // the current row only (which may not even be selected any more).
      const tags = commonTags(selectedSamples());
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
    toggleSelectedFavorites();
    event.preventDefault();
    return;
  }
  // Ctrl+A, or Cmd+A on Mac, selects every sample of the list, the rows off
  // screen included; in a text box it keeps selecting the text.
  if ((event.ctrlKey || event.metaKey) && event.key === "a" && !(event.target instanceof HTMLInputElement)) {
    selectAll();
    event.preventDefault();
    return;
  }
  // Ctrl+I, or Cmd+I on Mac, folds or unfolds the Info panel.
  if ((event.ctrlKey || event.metaKey) && event.key === "i") {
    toggleInfo();
    event.preventDefault();
    return;
  }
  // Ctrl+B, or Cmd+B on Mac, folds or unfolds the collections column.
  if ((event.ctrlKey || event.metaKey) && event.key === "b") {
    toggleCollections();
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
      select(selected + 1, event.shiftKey ? "range" : "single");
      break;
    case "ArrowUp":
      select(selected - 1, event.shiftKey ? "range" : "single");
      break;
    case "Enter":
      play();
      break;
    case " ":
      // In a text box Space types. Elsewhere it plays or pauses, even with
      // the focus left on a slider, a checkbox or a button just used, which
      // would otherwise take it (ticking the box, pressing the button again).
      if (isTextEntry(event.target)) return;
      playOrPause();
      break;
    case "l":
    case "L":
      if (isTextEntry(event.target) || event.ctrlKey || event.metaKey || event.altKey) {
        return;
      }
      // Through the checkbox itself: its change handler restarts the sound
      // in the new mode and remembers the choice.
      loopBox.click();
      break;
    case "Escape":
      stop();
      // A right-click menu still waiting for its answer must not open.
      closeContextMenu();
      break;
    default:
      return;
  }
  event.preventDefault();
});

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

// A press outside a window (Settings, Folders) closes it. Only the press
// counts: selecting text in a box and releasing the mouse outside must not
// close it. The backdrop belongs to the <dialog> itself, as do its own
// margins, hence the position check.
function closeOnPressOutside(dialog) {
  dialog.addEventListener("mousedown", (event) => {
    if (event.target !== dialog) return;
    const box = dialog.getBoundingClientRect();
    const inside =
      event.clientX >= box.left &&
      event.clientX <= box.right &&
      event.clientY >= box.top &&
      event.clientY <= box.bottom;
    if (!inside) dialog.close();
  });
}

closeOnPressOutside(settingsDialog);
closeOnPressOutside(foldersDialog);

document.getElementById("show-settings").addEventListener("click", () => {
  renderSettings();
  settingsDialog.showModal();
});

applyColumns();
applyColumnWidths();
// The types are needed to show a subtype with its family: asked first.
invoke("sound_types")
  .then((families) => {
    soundFamilies = families;
  })
  .catch(showError)
  .finally(search);
loadCollections();
loadColors();
loadTagList();
offerSpliceFolders();
