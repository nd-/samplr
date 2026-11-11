// The current sample's details: the Info panel, the playback bar's line,
// and the waveform loaded for it.

// The Info panel, on the right: the current sample's details, what the list
// knows (format, tempo, key, loop and where they come from) and what the
// file says of itself (`file_info`: size, date, its own metadata). Drawn
// again whenever the selection or the samples change, with `renderTags`;
// the file is read again only when the current sample is another one.
const infoColumn = document.getElementById("info");
const infoList = document.getElementById("info-list");
let infoFor = null;
let infoOfFile = null;

const tempoOrigins = {
  name: "from the name",
  file: "from the file",
  length: "estimated from the length",
  sound: "detected from the sound",
  manual: "set by hand",
};

// A tempo worked out rather than read: shown "≈ 100", greyed.
function tempoEstimated(sample) {
  return sample.bpm_source === "length" || sample.bpm_source === "sound";
}

// Where a sample's tempo comes from, with how it was found when estimated:
// "detected from the sound: 8 beats in 4.00 s".
function tempoOrigin(sample) {
  let origin = tempoOrigins[sample.bpm_source];
  if (sample.bpm_fixed) origin += ", chosen with Detect BPM";
  if (!tempoEstimated(sample) || !(sample.duration_secs > 0)) return origin;
  const beats = Math.round((sample.bpm * sample.duration_secs) / 60);
  return `${origin}: ${beats} beats in ${sample.duration_secs.toFixed(2)} s`;
}

async function renderInfo() {
  const sample = samples[selected];
  renderPlayer(sample);
  // Folded, the file is not read: it is when the panel opens again.
  if (infoColumn.classList.contains("collapsed")) {
    infoFor = null;
    return;
  }
  if (!sample) {
    infoFor = null;
    infoOfFile = null;
    const hint = document.createElement("p");
    hint.className = "hint";
    hint.textContent = "Select a sample to see its details.";
    infoList.replaceChildren(hint);
    return;
  }
  if (infoFor !== sample.path) {
    infoFor = sample.path;
    infoOfFile = null;
    drawInfo(sample);
    try {
      const found = await invoke("file_info", { path: sample.path });
      // The selection may have moved on meanwhile.
      if (infoFor !== sample.path) return;
      infoOfFile = found;
    } catch (error) {
      if (infoFor === sample.path) infoOfFile = { error: String(error) };
    }
  }
  drawInfo(samples[selected] ?? sample);
}

function drawInfo(sample) {
  const items = [];
  const add = (label, value, origin) => {
    if (value === null || value === undefined || value === "") return;
    const term = document.createElement("dt");
    term.textContent = label;
    const description = document.createElement("dd");
    description.textContent = value;
    if (origin) {
      const small = document.createElement("span");
      small.className = "origin";
      small.textContent = ` ${origin}`;
      description.append(small);
    }
    items.push(term, description);
  };
  const heading = (text) => {
    const title = document.createElement("h3");
    title.textContent = text;
    items.push(title);
  };
  const { folder, name } = splitPath(sample.path);
  add("Name", name);
  add("Folder", folder);
  add("Format", formatOf(sample));
  add(
    "Length",
    sample.duration_secs === null
      ? null
      : [formatDuration(sample.duration_secs), barsText(sample)].filter(Boolean).join(" · "),
  );
  if (infoOfFile && !infoOfFile.error) {
    add("Size", formatSize(infoOfFile.size));
    if (infoOfFile.modified !== null) {
      add("Modified", new Date(infoOfFile.modified * 1000).toLocaleString());
    }
  }
  add("Tempo", sample.bpm === null ? null : `${sample.bpm} BPM`, tempoOrigin(sample));
  add("Key", sample.key);
  add("Loudness", loudnessText(sample) || null);
  add(
    "Loop",
    sample.loop_source ? (sample.looped ? "Loop" : "One-shot") : null,
    loopOrigins[sample.loop_source],
  );
  add("Type", typeLabel(sample) || null, typeOrigins[sample.type_source]);
  if (sample.tags.length > 0) add("Tags", sample.tags.join(", "));
  if (selectedPaths.size > 1) add("Selection", `${selectedPaths.size} samples`);
  if (infoOfFile?.error) {
    heading("In the file");
    add("Error", infoOfFile.error);
  } else if (infoOfFile && infoOfFile.tags.length > 0) {
    heading("In the file");
    for (const [label, value] of infoOfFile.tags) add(label, value);
  }
  infoList.replaceChildren(...items);
}

// The playback bar: the selected sample's name, then its tempo, key, loop,
// length and loudness on one line.
const nowName = document.getElementById("now-name");
const nowDetails = document.getElementById("now-details");

function renderPlayer(sample) {
  if (!sample) {
    nowName.textContent = "";
    nowDetails.textContent = "";
    return;
  }
  nowName.textContent = splitPath(sample.path).name;
  nowName.title = sample.path;
  const details = [];
  // Pitched or played at another tempo: the tempo and key heard, the
  // file's own between brackets.
  const shape = shapeFor(sample);
  if (sample.bpm !== null) {
    const estimated = tempoEstimated(sample) ? "≈ " : "";
    const bpm = Math.round(sample.bpm * shape.speed * shape.tempo * 10) / 10;
    details.push(estimated + (bpm === sample.bpm ? `${bpm} BPM` : `${bpm} BPM (${sample.bpm})`));
  }
  if (sample.key) {
    const heard = transpose(sample.key, Math.round(heardSemitones(sample)));
    details.push(heard === sample.key ? heard : `${heard} (${sample.key})`);
  }
  if (sample.loop_source) details.push(sample.looped ? "Loop" : "One-shot");
  if (sample.duration_secs !== null) {
    const bars = barsText(sample);
    details.push(formatDuration(sample.duration_secs) + (bars ? ` (${bars})` : ""));
  }
  if (sample.lufs !== null) details.push(`${sample.lufs.toFixed(1)} LUFS`);
  nowDetails.textContent = details.join(" · ");
}

const notes = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

// "Am" up 2 semitones: "Bm". Keys are written in sharps.
function transpose(key, steps) {
  const match = /^([A-G]#?)(.*)$/.exec(key);
  if (!match || steps === 0) return key;
  const index = notes.indexOf(match[1]);
  return notes[(((index + steps) % 12) + 12) % 12] + match[2];
}

// A loop's length in bars of four beats, from its tempo: "4 bars", "1 bar",
// "2 beats" under a bar, "≈ 4.5 bars" when it is not a whole number (a tail
// of reverb, a tempo estimated wrong). Empty for a one-shot or without a
// tempo.
function barsText(sample) {
  if (!sample.looped || !sample.bpm || !(sample.duration_secs > 0)) return "";
  const beats = (sample.bpm * sample.duration_secs) / 60;
  if (beats < 3.9) {
    const whole = Math.round(beats);
    return Math.abs(beats - whole) < 0.1 && whole > 0
      ? `${whole} beat${whole === 1 ? "" : "s"}`
      : `≈ ${beats.toFixed(1)} beats`;
  }
  const bars = beats / 4;
  const whole = Math.round(bars);
  if (Math.abs(bars - whole) < 0.05) return `${whole} bar${whole === 1 ? "" : "s"}`;
  return `≈ ${bars.toFixed(1)} bars`;
}

// "WAV · pcm_s24le · 24 bit · 44.1 kHz · stereo".
function formatOf(sample) {
  const extension = sample.path.slice(sample.path.lastIndexOf(".") + 1).toUpperCase();
  const parts = [extension];
  if (sample.codec) parts.push(sample.codec);
  if (sample.bits) parts.push(`${sample.bits} bit`);
  if (sample.sample_rate) parts.push(`${sample.sample_rate / 1000} kHz`);
  if (sample.channels) {
    parts.push(sample.channels === 1 ? "mono" : sample.channels === 2 ? "stereo" : `${sample.channels} channels`);
  }
  return parts.join(" · ");
}

function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

const infoToggle = document.getElementById("toggle-info");

function setInfoFolded(folded) {
  infoColumn.classList.toggle("collapsed", folded);
  infoToggle.textContent = folded ? "‹" : "›";
  infoToggle.title = shortcut(folded ? "Show the info (Ctrl+I)" : "Hide the info (Ctrl+I)");
  // The waveform and the list take the width freed or taken.
  drawWaveform();
  applyColumnWidths();
  store("infoFolded", folded);
  if (!folded) renderInfo();
}

function toggleInfo() {
  setInfoFolded(!infoColumn.classList.contains("collapsed"));
}

infoToggle.addEventListener("click", toggleInfo);

// Called after a change with each sample's new tags (same order): the list
// rows and the tag bar show them.
function showTags(changed, allTags) {
  changed.forEach((sample, i) => {
    sample.tags = allTags[i];
  });
  // A tag added may be coloured: the rows are drawn again (a screenful).
  drawRows();
  renderTags();
  // A tag may have appeared, gone, or changed count.
  loadTagList();
}

// A short light on what just changed, so that one sees the action took.
function flash(elements) {
  const shown = elements.filter(Boolean);
  for (const element of shown) element.classList.remove("flash");
  // Reading a size makes the browser apply the removals now: the animation
  // then starts again even if it was still running. Once for all of them.
  void document.body.offsetWidth;
  for (const element of shown) {
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
    flash([...rowsOf(paths), tagBar]);
  } catch (error) {
    showError(error);
  }
}

async function removeTag(tag) {
  const chosen = selectedSamples();
  if (chosen.length === 0) return;
  try {
    const paths = chosen.map((sample) => sample.path);
    showTags(chosen, await invoke("remove_tag", { paths, tag }));
  } catch (error) {
    showError(error);
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
    showError(error);
  }
  drawWaveform();
}
