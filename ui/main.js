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
    tr.append(
      cell(name),
      cell(folder, "folder"),
      cell(formatDuration(sample.duration_secs), "number"),
      cell(sample.tags.join(" "), "tags"),
    );
    tr.addEventListener("click", () => select(index));
    tr.addEventListener("mousedown", (event) => watchForDrag(event, sample));
    rows.append(tr);
  });
  tbody.replaceChildren(rows);
  status.textContent = `${samples.length} sample(s)`;
}

function select(index) {
  if (index < 0 || index >= samples.length) return;
  tbody.children[selected]?.classList.remove("selected");
  selected = index;
  const row = tbody.children[selected];
  row.classList.add("selected");
  row.scrollIntoView({ block: "nearest" });
  play();
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

function play() {
  const sample = samples[selected];
  if (!sample) return;
  status.textContent = `▶ ${splitPath(sample.path).name}`;
  stopButton.disabled = false;
  invoke("play", { path: sample.path }).catch((error) => {
    status.textContent = `Error: ${error}`;
  });
}

function stop() {
  invoke("stop");
  stopButton.disabled = true;
  status.textContent = `${samples.length} sample(s)`;
}

stopButton.addEventListener("click", stop);

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

async function search() {
  try {
    samples = await invoke("search", { query: searchBox.value });
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
