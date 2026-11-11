// What every part of the page shares: the Tauri API, shortcuts shown for
// the Mac, the choices kept on this computer, errors, and the samples listed.
// The page's scripts are classic ones, loaded in order by index.html: a
// function is only there once its file has run.

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
