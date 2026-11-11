// The page's start, once every other script has run.

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
