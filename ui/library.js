// The library: dragging samples out, adding and removing folders, and the
// list kept up to date with the disk.

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
    // A refresh since the button went down may have taken the sample out of
    // the list (deleted, renamed): its old path would be dragged.
    const current = samples.find((s) => s.path === sample.path);
    if (current) startDrag(current);
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
