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
const tableHead = document.querySelector("thead");
const overscan = 10;
let rowHeight = 0;
// The samples whose rows are in the table: `firstDrawn` to `lastDrawn`.
let firstDrawn = 0;
let lastDrawn = -1;

function spacer(height) {
  const tr = document.createElement("tr");
  tr.className = "spacer";
  const td = document.createElement("td");
  td.colSpan = 7;
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
  const star = cell(sample.favorite ? "★" : "☆", "star");
  star.title = "Favorite (Ctrl+D)";
  tr.append(
    star,
    cell(name),
    cell(folder, "folder"),
    cell(formatDuration(sample.duration_secs), "number"),
    collectionsCell(sample.collections),
    tagsCell(sample.tags),
  );
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

// Scrolls just enough for the row to show below the sticky header.
function ensureVisible(index) {
  if (index < 0 || rowHeight === 0) return;
  const top = index * rowHeight;
  const bottom = tableHead.offsetHeight + top + rowHeight;
  if (top < listBox.scrollTop) listBox.scrollTop = top;
  else if (bottom > listBox.scrollTop + listBox.clientHeight) {
    listBox.scrollTop = bottom - listBox.clientHeight;
  }
  drawRowsIfNeeded();
}

function render() {
  drawRows();
  removeHeader.hidden = currentCollection === null;
  // The column of the × exists only in a collection. Its <col> is taken out
  // rather than given no width: WebKit treats a zero-width <col> as an
  // automatic one, which then shares the free width with Tags.
  if (currentCollection === null) removeColumn.remove();
  else columnGroup.append(removeColumn);
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
  if (index < 0) return;
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
  // Cmd on a Mac, Ctrl elsewhere (Ctrl+click is a right-click on a Mac).
  if (event.metaKey || event.ctrlKey) select(index, "toggle");
  else if (event.shiftKey) select(index, "range");
  else select(index);
});

tbody.addEventListener("mousedown", (event) => {
  const index = rowIndex(event);
  if (index >= 0) watchForDrag(event, samples[index]);
});

const removeHeader = document.querySelector("th.remove");
const removeColumn = document.querySelector("col.col-remove");
const columnGroup = removeColumn.parentElement;

// Column widths, changed by dragging the handle on a header's right edge;
// a double-click on it brings the default back. Remembered between
// sessions, on this computer only. Tags takes the rest of the width.
const defaultWidths = { name: 280, folder: 320, length: 90, collections: 160 };
let columnWidths = { ...defaultWidths };
try {
  columnWidths = { ...defaultWidths, ...JSON.parse(localStorage.getItem("columnWidths") ?? "{}") };
} catch {
  // Storage unavailable or unreadable: the default widths.
}

// The table always fills the list's width, no more: the widths shown are
// the chosen ones, shrunk together when they do not fit, with at least
// `minTagsWidth` left for Tags. The chosen widths are kept, so a wider window
// brings them back.
const minTagsWidth = 80;

// The width Name, Folder and Length may share.
function availableWidth() {
  const star = document.querySelector("th.star").offsetWidth;
  const remove = removeHeader.hidden ? 0 : removeHeader.offsetWidth;
  return listBox.clientWidth - star - remove - minTagsWidth;
}

function applyColumnWidths() {
  const total = Object.values(columnWidths).reduce((sum, width) => sum + width, 0);
  const available = availableWidth();
  const scale = total > available && available > 0 ? available / total : 1;
  for (const [name, width] of Object.entries(columnWidths)) {
    document.querySelector(`col[data-col="${name}"]`).style.width =
      `${Math.floor(width * scale)}px`;
  }
}

window.addEventListener("resize", applyColumnWidths);

function saveColumnWidths() {
  try {
    localStorage.setItem("columnWidths", JSON.stringify(columnWidths));
  } catch {
    // Storage unavailable: the widths are simply not remembered.
  }
}

for (const handle of document.querySelectorAll(".resizer")) {
  const name = handle.dataset.col;
  handle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    // Shrunk to fit a narrow window, the columns start from the widths shown,
    // so that the one dragged does not jump.
    for (const other of Object.keys(columnWidths)) {
      columnWidths[other] = parseInt(document.querySelector(`col[data-col="${other}"]`).style.width);
    }
    const startX = event.clientX;
    const startWidth = columnWidths[name];
    // The pointer stays with the handle even when it leaves the header.
    handle.setPointerCapture(event.pointerId);
    handle.classList.add("dragging");
    const resize = (move) => {
      // No wider than the room the other columns leave.
      const others = Object.entries(columnWidths)
        .filter(([other]) => other !== name)
        .reduce((sum, [, width]) => sum + width, 0);
      const widest = Math.max(40, availableWidth() - others);
      columnWidths[name] = Math.min(widest, Math.max(40, startWidth + move.clientX - startX));
      applyColumnWidths();
    };
    handle.addEventListener("pointermove", resize);
    handle.addEventListener(
      "pointerup",
      () => {
        handle.removeEventListener("pointermove", resize);
        handle.classList.remove("dragging");
        saveColumnWidths();
      },
      { once: true },
    );
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
  samples.sort((a, b) => {
    const x = value(a);
    const y = value(b);
    if ((x === null) !== (y === null)) return x === null ? 1 : -1;
    const order = compareValues(x, y);
    // Equal values: by path, so that the order stays the same every time.
    return (sortAscending ? order : -order) || collator.compare(a.path, b.path);
  });
}

for (const header of document.querySelectorAll("th[data-sort]")) {
  header.addEventListener("click", () => {
    if (sortKey === header.dataset.sort) {
      sortAscending = !sortAscending;
    } else {
      sortKey = header.dataset.sort;
      sortAscending = true;
    }
    for (const other of document.querySelectorAll("th[data-sort]")) {
      other.classList.remove("sorted-ascending", "sorted-descending");
    }
    header.classList.add(sortAscending ? "sorted-ascending" : "sorted-descending");
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

// The tags of the selection, each with a button to remove it: with several
// samples, the tags they all carry.
function renderTags() {
  const chosen = selectedSamples();
  tagInput.disabled = chosen.length === 0;
  tagInput.placeholder = chosen.length > 1 ? `Add tags to ${chosen.length} samples` : "Add tags";
  const common = chosen.length === 0
    ? []
    : chosen[0].tags.filter((tag) => chosen.every((sample) => sample.tags.includes(tag)));
  const chips = common.map((tag) => {
    const chip = document.createElement("span");
    chip.className = "tag";
    chip.textContent = tag;
    paint(chip, tagColor(tag));
    chip.title = "Right-click to give it a colour";
    chip.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      openColorMenu(event.clientX, event.clientY, tagColorIds.get(tag) ?? null, async (color) => {
        await invoke("set_tag_color", { tag, color });
        await loadColors();
      });
    });
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.title = `Remove ${tag}`;
    remove.addEventListener("click", () => removeTag(tag));
    chip.append(remove);
    return chip;
  });
  tagList.replaceChildren(...chips);
}

// Called after a change with each sample's new tags (same order): the list
// rows and the tag bar show them.
function showTags(changed, allTags) {
  changed.forEach((sample, i) => {
    sample.tags = allTags[i];
    const row = rowElement(samples.indexOf(sample));
  });
  // A tag added may be coloured: the rows are drawn again (a screenful).
  drawRows();
  renderTags();
}

// A short light on what just changed, so that one sees the action took.
function flash(elements) {
  for (const element of elements) {
    if (!element) continue;
    element.classList.remove("flash");
    // Reading the size makes the browser apply the removal now: the
    // animation then starts again even if it was still running.
    void element.offsetWidth;
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
    flash([...rowsOf(paths), document.getElementById("tagbar")]);
  } catch (error) {
    status.textContent = `Error: ${error}`;
  }
}

async function removeTag(tag) {
  const chosen = selectedSamples();
  if (chosen.length === 0) return;
  try {
    const paths = chosen.map((sample) => sample.path);
    showTags(chosen, await invoke("remove_tag", { paths, tag }));
  } catch (error) {
    status.textContent = `Error: ${error}`;
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
// files are offered as copies, so the receiving app never moves the
// originals. Dragging a selected row takes the whole selection along.
async function startDrag(sample) {
  const paths = selectedPaths.has(sample.path)
    ? selectedSamples().map((chosen) => chosen.path)
    : [sample.path];
  const label = paths.length > 1 ? `${paths.length} samples` : splitPath(sample.path).name;
  try {
    await window.__TAURI__.drag.startDrag({
      item: paths,
      icon: dragImage(label),
      mode: "copy",
    });
  } catch (error) {
    status.textContent = `Error: ${error}`;
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
    sortSamples();
  } catch (error) {
    status.textContent = `Error: ${error}`;
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
    status.textContent = `Error: ${error}`;
    return;
  }
  for (const sample of chosen) {
    sample.favorite = favorite;
    const star = rowElement(samples.indexOf(sample))?.querySelector(".star");
    if (star) star.textContent = favorite ? "★" : "☆";
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
    status.textContent = `Error: ${error}`;
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

// `id`: the collection's, kept on the item to find it under a drop; none for
// "All samples", which takes no drop.
function collectionItem(label, count, active, open, id, hex = null) {
  const item = document.createElement("li");
  if (active) item.classList.add("active");
  if (id !== undefined) item.dataset.id = id;
  const name = document.createElement("span");
  name.className = "name";
  if (hex) name.append(dot(hex));
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
      colorHex(collection.color_id),
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
  await refresh({ stopIfGone: false });
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

// The samples the collection commands act on: the selection.
function chosenPaths() {
  return selectedSamples().map((sample) => sample.path);
}

// `paths`: by default the selection; the dropped files for a drop.
async function addToCollection(collection, paths = chosenPaths()) {
  if (paths.length === 0) return;
  let result;
  try {
    result = await invoke("add_to_collection", { collection: collection.id, paths });
  } catch (error) {
    status.textContent = `Error: ${error}`;
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
    status.textContent = `Error: ${error}`;
  }
}

// The collections column can be folded into a thin strip (button or Ctrl+B,
// Cmd+B on Mac); remembered between sessions, on this computer only.
const collectionsColumn = document.getElementById("collections");
const collectionsToggle = document.getElementById("toggle-collections");

function setCollectionsFolded(folded) {
  collectionsColumn.classList.toggle("collapsed", folded);
  collectionsToggle.textContent = folded ? "›" : "‹";
  collectionsToggle.title = folded ? "Show the collections (Ctrl+B)" : "Hide the collections (Ctrl+B)";
  // The waveform and the list take the width freed or taken.
  drawWaveform();
  applyColumnWidths();
  try {
    localStorage.setItem("collectionsFolded", String(folded));
  } catch {
    // Storage unavailable: the state is simply not remembered.
  }
}

function toggleCollections() {
  setCollectionsFolded(!collectionsColumn.classList.contains("collapsed"));
}

collectionsToggle.addEventListener("click", toggleCollections);

try {
  if (localStorage.getItem("collectionsFolded") === "true") setCollectionsFolded(true);
} catch {
  // Storage unavailable: the column starts unfolded.
}

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
// `hex`: a coloured dot before the label.
function menuButton(label, action, keepOpen = false, hex = null) {
  const button = document.createElement("button");
  if (hex) button.append(dot(hex));
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
async function openContextMenu(x, y) {
  menuPosition = { x, y };
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
      // Already there: the name cannot be clicked, a × next to it takes the
      // samples out.
      const already = menuButton(
        `✓ ${collection.name}`,
        () => {},
        false,
        colorHex(collection.color_id),
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
          colorHex(collection.color_id),
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
  const index = rowIndex(event);
  if (index < 0) return;
  event.preventDefault();
  // A row outside the selection becomes the only one selected; inside it,
  // the menu acts on the whole selection.
  if (!selectedPaths.has(samples[index].path)) select(index, "single", false);
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
  if (foldersDialog.open || settingsDialog.open) return;
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
    toggleSelectedFavorites();
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
    case "Escape":
      stop();
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
    palette = await invoke("colors");
    tagColorIds = new Map(await invoke("tag_colors"));
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  drawRows();
  renderTags();
  renderCollections();
}

// The palette's colours to choose from, plus "No colour"; `current` is
// marked. Used for a tag and for a collection.
function openColorMenu(x, y, current, choose) {
  const items = [];
  const none = menuButton(current === null ? "✓ No colour" : "No colour", () => choose(null));
  items.push(none);
  for (const color of palette) {
    const label = color.id === current ? `✓ ${color.name}` : color.name;
    items.push(menuButton(label, () => choose(color.id), false, color.hex));
  }
  contextMenu.replaceChildren(...items);
  contextMenu.hidden = false;
  const { width, height } = contextMenu.getBoundingClientRect();
  contextMenu.style.left = `${Math.min(x, window.innerWidth - width - 4)}px`;
  contextMenu.style.top = `${Math.min(y, window.innerHeight - height - 4)}px`;
}

// The Settings window: for now, the palette. Each change is saved at once.
const settingsDialog = document.getElementById("settings");
const colorList = document.getElementById("color-list");

function renderSettings() {
  const items = palette.map((color) => {
    const item = document.createElement("li");
    const picker = document.createElement("input");
    picker.type = "color";
    picker.value = color.hex;
    const name = document.createElement("input");
    name.type = "text";
    name.value = color.name;
    name.spellcheck = false;
    const save = async () => {
      try {
        await invoke("update_color", { id: color.id, name: name.value, hex: picker.value });
      } catch (error) {
        status.textContent = `Error: ${error}`;
        return;
      }
      await loadColors();
    };
    // `change`: when the picker closes or the name box is left, not at
    // every step.
    picker.addEventListener("change", save);
    name.addEventListener("change", save);
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.title = `Delete ${color.name}`;
    remove.addEventListener("click", () => deleteColor(color));
    item.append(picker, name, remove);
    return item;
  });
  colorList.replaceChildren(...items);
}

async function deleteColor(color) {
  try {
    const [tags, collectionCount] = await invoke("color_usage", { id: color.id });
    if (tags + collectionCount > 0) {
      const confirmed = await window.__TAURI__.dialog.ask(
        `Delete the colour ${color.name}?\n\n` +
          `${tags} tag(s) and ${collectionCount} collection(s) use it; ` +
          "they will be left without colour.",
        { title: "Delete colour", kind: "warning" },
      );
      if (!confirmed) return;
    }
    await invoke("delete_color", { id: color.id });
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  await loadColors();
  await loadCollections();
  renderSettings();
}

document.getElementById("add-color").addEventListener("click", async () => {
  try {
    await invoke("add_color", { name: "New colour", hex: "#888888" });
  } catch (error) {
    status.textContent = `Error: ${error}`;
    return;
  }
  await loadColors();
  renderSettings();
  colorList.lastElementChild?.querySelector('input[type="text"]').select();
});

document.getElementById("show-settings").addEventListener("click", () => {
  renderSettings();
  settingsDialog.showModal();
});

search();
loadCollections();
loadColors();
