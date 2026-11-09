// Applies the theme chosen in Settings before the page is first drawn, so
// that it does not flash in the default colours. Loaded in <head>, before
// main.js. "system" (or nothing stored) leaves the system's light or dark.
try {
  const theme = localStorage.getItem("theme");
  if (theme && theme !== "system") document.documentElement.dataset.theme = theme;
} catch {
  // Storage unavailable: the system's light or dark.
}
