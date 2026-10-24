// `withGlobalTauri` in tauri.conf.json exposes the Tauri API as
// `window.__TAURI__`, so the page needs no build tool to import it.
const { invoke } = window.__TAURI__.core;

const count = document.getElementById("count");

invoke("sample_count")
  .then((n) => { count.textContent = `${n} sample(s) in the database.`; })
  .catch((error) => { count.textContent = `Error: ${error}`; });
