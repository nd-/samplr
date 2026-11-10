// A stand-in for the Tauri API, injected before the page's scripts: the
// page runs in a plain browser, its commands answered from a small library
// held here. Not the Rust side: each command does just enough for the page
// to behave as with the real one.
//
// For the tests: `window.__calls` records every command with its arguments,
// `window.__emit(name, payload)` sends an event as Rust would, and
// `window.__answer` is what `dialog.ask` answers (true by default).

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
  const collections = [];
  const palette = [
    { id: 1, name: "Red", hex: "#e5484d" },
    { id: 2, name: "Blue", hex: "#0090ff" },
  ];

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

  // Words in the path or a tag, `#tag` exact; the filters (bpm:, key:…)
  // belong to the Rust side and are not imitated.
  function search({ query, favorites, collection }) {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return samples
      .filter((sample) => !favorites || sample.favorite)
      .filter((sample) => collection === null || sample.collections.includes(collection))
      .filter((sample) =>
        words.every((word) =>
          word.startsWith("#")
            ? sample.tags.includes(word.slice(1))
            : sample.path.toLowerCase().includes(word) ||
              sample.tags.some((tag) => tag.includes(word)),
        ),
      )
      .map((sample) => structuredClone(sample));
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
      { id: "bass", label: "Bass", subtypes: [] },
    ],
    search,
    collections: () =>
      collections.map((c) => ({
        ...c,
        samples: samples.filter((s) => s.collections.includes(c.id)).length,
      })),
    colors: () => palette,
    tag_colors: () => [],
    tags: () => tagCounts(),
    splice_folders: () => [],
    library_problems: () => [],
    ffmpeg_available: () => true,
    folders: () => [{ path: "/lib", samples: samples.length, found: true }],
    file_info: () => ({ size: 1000, modified: 0, tags: [] }),
    waveform: () => Array.from({ length: 1024 }, (_, i) => [-Math.abs(Math.sin(i / 20)), Math.abs(Math.sin(i / 20))]),
    play: () => null,
    stop: () => null,
    pause: () => null,
    set_volume: () => null,
    set_playback: () => null,
    collections_holding: () => [],
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
    create_collection: ({ name }) => {
      const id = collections.length + 1;
      collections.push({ id, name, color_id: null });
      return id;
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

  const listeners = new Map();
  window.__calls = [];
  window.__answer = true;
  window.__emit = (name, payload) => {
    for (const handler of listeners.get(name) ?? []) handler({ event: name, payload });
  };

  window.__TAURI__ = {
    core: {
      invoke: async (command, args = {}) => {
        window.__calls.push({ command, args: structuredClone(args) });
        const run = commands[command];
        if (!run) throw `the fake Tauri API has no command “${command}”`;
        return structuredClone(run(args));
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
      ask: async () => window.__answer,
      open: async () => null,
    },
    drag: { startDrag: async () => null },
    webview: {
      getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
    },
  };
})();
