// The sample list: drawing the rows, columns, widths, sorting, selection,
// and the tag bar.

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
