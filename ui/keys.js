// Searching, and the keys of the list.

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
