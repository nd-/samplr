// Favorites, collections, the Tags box, and the left column's layout and
// drops.

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
