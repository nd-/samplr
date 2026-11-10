// A stand-in for the Tauri API, injected before the page's scripts: the
// page runs in a plain browser, its commands answered from a small library
// held here. Not the Rust side: each command does just enough for the page
// to behave as with the real one.
//
// For the tests: `window.__calls` records every command with its arguments,
// `window.__emit(name, payload)` sends an event as Rust would, and
// `window.__answer` is what `dialog.ask` answers (true by default), each
// question kept in `window.__asked`.
// `window.__library` changes the library as the folder watcher would (the
// test then emits `library-changed`), `window.__delay(command, args)` gives
// an answer's delay in milliseconds, and `window.__problems` holds what
// `library_problems` reports.

(() => {
  const names = [];
  for (let i = 1; i <= 12; i++) names.push(`Drums/Kick ${i}.wav`);
  for (let i = 1; i <= 12; i++) names.push(`Drums/Snare ${i}.wav`);
  for (let i = 1; i <= 13; i++) names.push(`Loops/Bass ${i} 120BPM Am.wav`);
  for (let i = 1; i <= 13; i++) names.push(`Pads/Pad ${i}.wav`);

  const samples = names.map((name, i) => {
    const loop = name.startsWith("Loops/");
    return {
      path: `/lib/${name}`,
      duration_secs: loop ? 2 : 0.5 + (i % 7) / 10,
      sample_rate: 44100,
      channels: i % 3 === 0 ? 1 : 2,
      bits: 24,
      tags: [],
      favorite: false,
      collections: [],
      bpm: loop ? 120 : null,
      bpm_source: loop ? "name" : null,
      bpm_fixed: false,
      key: loop ? "Am" : null,
      looped: loop,
      loop_source: "name",
      sound_type: name.startsWith("Drums/Kick") ? "drums/kick" : null,
      type_source: name.startsWith("Drums/Kick") ? "name" : null,
      lufs: -12 - (i % 10),
      peak_db: -1,
      codec: "pcm_s24le",
    };
  });
  const byPath = new Map(samples.map((sample) => [sample.path, sample]));
  // The type worked out from each path, what "Automatic" goes back to.
  const automaticType = new Map(samples.map((sample) => [sample.path, sample.sound_type]));
  const collections = [];
  const palette = [
    { id: 1, name: "Red", hex: "#e5484d" },
    { id: 2, name: "Blue", hex: "#0090ff" },
  ];
  // Tag name → colour id.
  const tagColors = new Map();

  function color(id) {
    const found = palette.find((c) => c.id === id);
    if (!found) throw `no colour ${id}`;
    return found;
  }

  function checkHex(hex) {
    if (!/^#[0-9a-f]{6}$/i.test(hex.trim())) throw `invalid colour: "${hex}" (expected #rrggbb)`;
    return hex.trim().toLowerCase();
  }

  function chosen(paths) {
    return paths.map((path) => {
      const sample = byPath.get(path);
      if (!sample) throw `${path} is not in the database`;
      return sample;
    });
  }

  function tagCounts() {
    const counts = new Map();
    for (const sample of samples) {
      for (const tag of sample.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
    return [...counts].sort(([a], [b]) => a.localeCompare(b));
  }

  // Words in the path or a tag, `#tag` exact, `type:` a family or a subtype;
  // the other filters (bpm:, key:…) belong to the Rust side and are not
  // imitated.
  function search({ query, favorites, collection }) {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return samples
      .filter((sample) => !favorites || sample.favorite)
      .filter((sample) => collection === null || sample.collections.includes(collection))
      .filter((sample) =>
        words.every((word) =>
          word.startsWith("type:")
            ? typeMatches(sample.sound_type, word.slice(5))
            : word.startsWith("#")
            ? sample.tags.includes(word.slice(1))
            : sample.path.toLowerCase().includes(word) ||
              sample.tags.some((tag) => tag.includes(word)),
        ),
      )
      .map((sample) => structuredClone(sample));
  }

  function typeMatches(value, wanted) {
    if (!value) return false;
    const [family, subtype] = value.split("/");
    return wanted === value || wanted === family || wanted === subtype;
  }

  const commands = {
    sound_types: () => [
      {
        id: "drums",
        label: "Drums",
        subtypes: [
          { id: "kick", label: "kick" },
          { id: "snare", label: "snare" },
        ],
      },
      { id: "bass", label: "Bass", subtypes: [{ id: "electric", label: "electric" }] },
      {
        id: "guitar",
        label: "Guitar",
        subtypes: [
          { id: "acoustic", label: "acoustic" },
          { id: "electric", label: "electric" },
        ],
      },
    ],
    search,
    // Sorted by name, case ignored, as the database does.
    collections: () =>
      [...collections].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" })).map((c) => ({
        ...c,
        samples: samples.filter((s) => s.collections.includes(c.id)).length,
      })),
    colors: () => palette,
    tag_colors: () => [...tagColors],
    add_color: ({ name, hex }) => {
      const id = Math.max(0, ...palette.map((c) => c.id)) + 1;
      palette.push({ id, name: name.trim(), hex: checkHex(hex) });
      return id;
    },
    update_color: ({ id, name, hex }) => {
      Object.assign(color(id), { name: name.trim(), hex: checkHex(hex) });
      return null;
    },
    color_usage: ({ id }) => [
      [...tagColors.values()].filter((c) => c === id).length,
      collections.filter((c) => c.color_id === id).length,
    ],
    delete_color: ({ id }) => {
      palette.splice(palette.indexOf(color(id)), 1);
      for (const [tag, c] of tagColors) if (c === id) tagColors.delete(tag);
      for (const collection of collections) if (collection.color_id === id) collection.color_id = null;
      return null;
    },
    set_tag_color: ({ tag, color }) => {
      if (color === null) tagColors.delete(tag);
      else tagColors.set(tag, color);
      return null;
    },
    set_collection_color: ({ collection, color }) => {
      collections.find((c) => c.id === collection).color_id = color;
      return null;
    },
    tags: () => tagCounts(),
    splice_folders: () => [],
    library_problems: () => window.__problems.splice(0),
    ffmpeg_available: () => true,
    folders: () => [{ path: "/lib", samples: samples.length, found: true }],
    file_info: () => ({ size: 1000, modified: 0, tags: [] }),
    waveform: () => Array.from({ length: 1024 }, (_, i) => [-Math.abs(Math.sin(i / 20)), Math.abs(Math.sin(i / 20))]),
    play: () => null,
    stop: () => null,
    pause: () => null,
    set_volume: () => null,
    set_playback: () => null,
    // The collections holding every one of the samples.
    collections_holding: ({ paths }) => {
      const held = chosen(paths);
      return collections
        .map((c) => c.id)
        .filter((id) => held.length > 0 && held.every((sample) => sample.collections.includes(id)));
    },
    set_favorite: ({ paths, favorite }) => {
      for (const sample of chosen(paths)) sample.favorite = favorite;
      return null;
    },
    add_tags: ({ paths, tags }) =>
      chosen(paths).map((sample) => {
        for (const tag of tags) {
          const name = tag.trim().toLowerCase();
          if (name && !sample.tags.includes(name)) sample.tags.push(name);
        }
        sample.tags.sort();
        return sample.tags;
      }),
    remove_tag: ({ paths, tag }) =>
      chosen(paths).map((sample) => {
        sample.tags = sample.tags.filter((t) => t !== tag);
        return sample.tags;
      }),
    set_tempo: ({ paths, value }) => {
      const text = value.trim();
      const bpm = Number(text);
      if (!["", "-", "auto"].includes(text) && !(Number.isInteger(bpm) && bpm >= 20 && bpm <= 400)) {
        throw `“${text}” is not a tempo between 20 and 400`;
      }
      for (const sample of chosen(paths)) {
        sample.bpm = text === "" || text === "-" ? null : text === "auto" ? sample.bpm : bpm;
        sample.bpm_source = sample.bpm === null ? null : "manual";
      }
      return null;
    },
    // "" back to the type worked out, "-" none, else a type set by hand.
    set_sound_type: ({ paths, value }) => {
      for (const sample of chosen(paths)) {
        if (value === "") {
          sample.sound_type = automaticType.get(sample.path);
          sample.type_source = sample.sound_type ? "name" : null;
        } else {
          sample.sound_type = value === "-" ? null : value;
          sample.type_source = "manual";
        }
      }
      return null;
    },
    create_collection: ({ name }) => {
      if (collections.some((c) => c.name === name)) {
        throw `a collection named "${name}" already exists`;
      }
      const id = Math.max(0, ...collections.map((c) => c.id)) + 1;
      collections.push({ id, name, color_id: null });
      return id;
    },
    delete_collection: ({ collection }) => {
      collections.splice(collections.findIndex((c) => c.id === collection), 1);
      for (const sample of samples) {
        sample.collections = sample.collections.filter((id) => id !== collection);
      }
      return null;
    },
    remove_from_collection: ({ collection, paths }) => {
      let removed = 0;
      for (const sample of chosen(paths)) {
        if (sample.collections.includes(collection)) {
          sample.collections = sample.collections.filter((id) => id !== collection);
          removed += 1;
        }
      }
      return removed;
    },
    add_to_collection: ({ collection, paths }) => {
      let added = 0;
      for (const sample of chosen(paths)) {
        if (!sample.collections.includes(collection)) {
          sample.collections.push(collection);
          added += 1;
        }
      }
      return { added, unknown: 0 };
    },
  };

  window.__library = {
    add(path) {
      const sample = structuredClone(samples[0]);
      Object.assign(sample, { path, tags: [], favorite: false, collections: [] });
      samples.push(sample);
      byPath.set(path, sample);
    },
    remove(path) {
      samples.splice(samples.indexOf(byPath.get(path)), 1);
      byPath.delete(path);
    },
    tag(path, tag) {
      byPath.get(path).tags.push(tag);
    },
  };
  window.__problems = [];
  window.__delay = () => 0;

  const listeners = new Map();
  window.__calls = [];
  window.__answer = true;
  window.__asked = [];
  window.__emit = (name, payload) => {
    for (const handler of listeners.get(name) ?? []) handler({ event: name, payload });
  };

  window.__TAURI__ = {
    core: {
      invoke: async (command, args = {}) => {
        window.__calls.push({ command, args: structuredClone(args) });
        const run = commands[command];
        if (!run) throw `the fake Tauri API has no command “${command}”`;
        const answer = structuredClone(run(args));
        const delay = window.__delay(command, args);
        if (delay > 0) await new Promise((resolve) => setTimeout(resolve, delay));
        return answer;
      },
    },
    event: {
      listen: async (name, handler) => {
        if (!listeners.has(name)) listeners.set(name, []);
        listeners.get(name).push(handler);
        return () => {};
      },
    },
    dialog: {
      ask: async (message) => {
        window.__asked.push(message);
        return window.__answer;
      },
      open: async () => null,
    },
    drag: { startDrag: async () => null },
    webview: {
      getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
    },
  };
})();
