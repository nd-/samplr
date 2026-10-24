// `withGlobalTauri` in tauri.conf.json exposes the Tauri API as
// `window.__TAURI__`, so the page needs no build tool to import it.
const { invoke } = window.__TAURI__.core;

const searchBox = document.getElementById("search");
const tbody = document.getElementById("samples");
const status = document.getElementById("status");
const canvas = document.getElementById("wave");

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
  invoke("play", { path: sample.path }).catch((error) => {
    status.textContent = `Error: ${error}`;
  });
}

async function search() {
  try {
    samples = await invoke("search", { query: searchBox.value });
    selected = -1;
    peaks = [];
    drawWaveform();
    render();
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
      invoke("stop");
      status.textContent = `${samples.length} sample(s)`;
      break;
    default:
      return;
  }
  event.preventDefault();
});

search();
