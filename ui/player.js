// Playback: the waveform drawn, play and pause, the playhead, loop, volume,
// pitch and tempo.

// A colour variable of the theme as `#rrggbb`. The stylesheet may write it
// `light-dark(…)` (the System theme) or `color-mix(…)`, which a canvas does
// not read: it then drew in black. The browser resolves it on an element.
function cssColor(name) {
  const probe = document.createElement("span");
  probe.style.color = `var(${name})`;
  document.body.append(probe);
  const computed = getComputedStyle(probe).color;
  probe.remove();
  // "rgb(196, 122, 8)", or "color(srgb 0.77 0.48 0.03)" for a mixed colour.
  const scale = computed.startsWith("color(") ? 255 : 1;
  const channels = computed.match(/[\d.]+/g).slice(0, 3).map((c) => Math.round(Number(c) * scale));
  return `#${channels.map((c) => c.toString(16).padStart(2, "0")).join("")}`;
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

  context.fillStyle = cssColor("--wave");
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

// A click on the waveform plays the sample from that point, whether it was
// playing or not.
canvas.addEventListener("click", (event) => {
  const duration = samples[selected]?.duration_secs;
  if (!(duration > 0)) return;
  const fraction = event.offsetX / canvas.clientWidth;
  play(Math.min(Math.max(fraction, 0), 1) * duration);
});

// Each sound played gets a new number: the end of a sound already replaced
// by another one may still be reported, and must then be ignored.
let playId = 0;

// The path of the sample playing, or `null`.
let playingPath = null;
// The sound playing is paused (Space): it resumes where it was.
let paused = false;

// Space: a loop, or any sample played with Loop on, pauses and resumes; a
// one-shot played once plays again from the start, as a pad would; nothing
// playing, the selected sample plays.
function playOrPause() {
  const sample = samples[selected];
  if (playingPath === null || (sample && sample.path !== playingPath)) {
    play();
    return;
  }
  if (sample && !sample.looped && !loopBox.checked && !paused) {
    play();
    return;
  }
  paused = !paused;
  invoke("pause", { paused }).catch(showError);
  updatePlayButton();
  // The playhead stops where the sound is, and goes on from there.
  if (lastPosition !== null) {
    const now = performance.now();
    if (paused) {
      lastPosition = {
        seconds: lastPosition.seconds + ((now - lastPosition.at) / 1000) * playSpeed(),
        at: now,
      };
    } else {
      lastPosition = { ...lastPosition, at: now };
      if (!animating) {
        animating = true;
        requestAnimationFrame(movePlayhead);
      }
    }
  }
  const name = splitPath(playingPath).name;
  status.textContent = `${paused ? "⏸" : "▶"} ${name}`;
}

// `start`: where to start, in seconds from the beginning of the sample.
function play(start = 0) {
  const sample = samples[selected];
  if (!sample) return;
  playId += 1;
  playingPath = sample.path;
  playingSample = sample;
  previousSpeed = playSpeed();
  paused = false;
  updatePlayButton();
  startPlayhead(sample.duration_secs, start);
  status.textContent = `▶ ${splitPath(sample.path).name}`;
  stopButton.disabled = false;
  const shape = shapeFor(sample);
  invoke("play", { path: sample.path, id: playId, looping: loopBox.checked, start, ...shape }).catch((error) => {
    stopPlayhead();
    showError(error);
  });
}

// The audio thread reports the position every 100 ms; in between, the line
// moves on the clock, so that it glides instead of jumping.
const playhead = document.getElementById("playhead");
let playheadDuration = null;
// The last position reported: seconds from the start, and when it came.
let lastPosition = null;
// A single animation loop at a time, however many sounds are started.
let animating = false;

function startPlayhead(duration, start) {
  playheadDuration = duration;
  lastPosition = { seconds: start, at: performance.now() };
  // Unknown length (some MP3s): no line, its place could not be computed.
  playhead.hidden = !(duration > 0);
  if (!animating) {
    animating = true;
    requestAnimationFrame(movePlayhead);
  }
}

function stopPlayhead() {
  lastPosition = null;
  playhead.hidden = true;
}

function movePlayhead() {
  if (lastPosition === null || playhead.hidden) {
    animating = false;
    return;
  }
  if (paused) {
    // Still drawn where it stopped; nothing moves until Space again.
    animating = false;
    return;
  }
  let seconds = lastPosition.seconds + ((performance.now() - lastPosition.at) / 1000) * playSpeed();
  seconds = loopBox.checked
    ? seconds % playheadDuration
    : Math.min(seconds, playheadDuration);
  playhead.style.left = `${(seconds / playheadDuration) * 100}%`;
  requestAnimationFrame(movePlayhead);
}

window.__TAURI__.event.listen("playback-position", (event) => {
  if (event.payload.id !== playId || lastPosition === null) return;
  lastPosition = { seconds: event.payload.seconds, at: performance.now() };
});

function showStopped() {
  playingPath = null;
  paused = false;
  updatePlayButton();
  stopPlayhead();
  stopButton.disabled = true;
  status.textContent = `${samples.length} sample(s)`;
}

function stop() {
  invoke("stop");
  showStopped();
}

window.__TAURI__.event.listen("playback-ended", (event) => {
  if (event.payload === playId) showStopped();
});

stopButton.addEventListener("click", stop);

const playButton = document.getElementById("play");
playButton.addEventListener("click", () => {
  playOrPause();
  // Back to the list, so that the arrow keys and Space act there.
  playButton.blur();
});

// ⏸ while a sound plays, ▶ otherwise (stopped or paused).
function updatePlayButton() {
  const playing = playingPath !== null && !paused;
  playButton.textContent = playing ? "⏸" : "▶";
}

// "Play loop": every sample played loops until the box is unchecked.
// Remembered between sessions, like the volume.
const loopBox = document.getElementById("loop");
loopBox.checked = stored("loop") === "true";
loopBox.addEventListener("change", () => {
  store("loop", loopBox.checked);
  // A sound already playing cannot change mode: the selected one starts
  // again in the new mode; another one, no longer selected, stops.
  if (playingPath !== null) {
    if (playingPath === samples[selected]?.path) play();
    else stop();
  }
  loopBox.blur();
});

// The slider's position is squared: the ear hears loudness roughly on a
// logarithmic scale, and a linear slider would do almost nothing in its top
// half. The position is remembered between sessions, on this computer only.
const volumeSlider = document.getElementById("volume");

function applyVolume() {
  const position = volumeSlider.value / 100;
  invoke("set_volume", { volume: position * position });
  store("volume", volumeSlider.value);
}

const savedVolume = stored("volume");
if (savedVolume !== null) volumeSlider.value = savedVolume;
volumeSlider.addEventListener("input", applyVolume);
applyVolume();

// How the preview is played: Pitch and Fine transpose, BPM is the tempo the
// samples whose tempo is known are played at. Tape changes pitch and tempo
// together, as a varispeed (lighter, and sometimes the sound wanted);
// otherwise one changes without the other (Signalsmith Stretch, see
// `stretch.rs`). Only Tape is remembered: a library heard out of tune or out
// of tempo at the next launch would mislead.
const pitchSlider = document.getElementById("pitch");
const fineSlider = document.getElementById("fine");
const pitchValue = document.getElementById("pitch-value");
const fineValue = document.getElementById("fine-value");
const targetBox = document.getElementById("target-bpm");
const tapeBox = document.getElementById("tape");
// The sample whose sound is playing, for the playhead's speed.
let playingSample = null;

function semitones() {
  return Number(pitchSlider.value) + Number(fineSlider.value) / 100;
}

// The tempo factor that brings `sample` to the BPM asked; 1 without one.
function tempoRatio(sample) {
  const target = Number(targetBox.value);
  if (!(target > 0) || !sample?.bpm) return 1;
  return Math.min(4, Math.max(0.25, target / sample.bpm));
}

// What the audio thread is sent: `speed` changes both, as a tape; `tempo`
// and `semitones` one without the other.
function shapeFor(sample) {
  const ratio = tempoRatio(sample);
  if (tapeBox.checked) return { speed: ratio * 2 ** (semitones() / 12), tempo: 1, semitones: 0 };
  return { speed: 1, tempo: ratio, semitones: semitones() };
}

// How much faster than the file the sound goes: the playhead's speed.
function playSpeed() {
  const shape = shapeFor(playingSample ?? samples[selected]);
  return shape.speed * shape.tempo;
}

// The semitones heard: with Tape, the tempo's change moves the pitch too.
function heardSemitones(sample) {
  const shape = shapeFor(sample);
  return shape.semitones + 12 * Math.log2(shape.speed);
}

function signed(value) {
  return value > 0 ? `+${value}` : String(value);
}

function applyPlayback() {
  // The playhead goes on from where it is at the old speed.
  if (lastPosition !== null && !paused) {
    const now = performance.now();
    lastPosition = {
      seconds: lastPosition.seconds + ((now - lastPosition.at) / 1000) * previousSpeed,
      at: now,
    };
  }
  previousSpeed = playSpeed();
  pitchValue.textContent = `${signed(Number(pitchSlider.value))} st`;
  fineValue.textContent = `${signed(Number(fineSlider.value))} ct`;
  pitchValue.classList.toggle("shifted", pitchSlider.value !== "0");
  fineValue.classList.toggle("shifted", fineSlider.value !== "0");
  targetBox.classList.toggle("shifted", targetBox.value !== "");
  invoke("set_playback", shapeFor(playingSample ?? samples[selected])).catch(showError);
  renderPlayer(samples[selected]);
}

let previousSpeed = 1;
for (const slider of [pitchSlider, fineSlider]) {
  slider.addEventListener("input", applyPlayback);
  slider.addEventListener("dblclick", () => {
    slider.value = 0;
    applyPlayback();
  });
}
targetBox.addEventListener("input", applyPlayback);
targetBox.addEventListener("keydown", (event) => {
  // The list's keys (arrows, Space, L) must not react while typing a tempo.
  event.stopPropagation();
  if (event.key === "Enter" || event.key === "Escape") targetBox.blur();
});
tapeBox.checked = stored("tape") === "true";
tapeBox.addEventListener("change", () => {
  store("tape", tapeBox.checked);
  applyPlayback();
  tapeBox.blur();
});
applyPlayback();

// Autoplay off: a click on a row selects it without playing it, to tag it or
// drag it in silence. The arrow keys, Enter and Play still play.
const autoplayBox = document.getElementById("autoplay");
autoplayBox.checked = stored("autoplay") !== "false";
autoplayBox.addEventListener("change", () => {
  store("autoplay", autoplayBox.checked);
  // Back to the list, so that the arrow keys move in it again.
  autoplayBox.blur();
});
