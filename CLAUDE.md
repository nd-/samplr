# samplr: context for Claude

## The project

Desktop audio sample manager for **macOS and Linux**: listen to, tag and organise samples (for
musicians, alongside a DAW).

## How we work

- Move forward in **small steps that compile and run**: each step must be testable with
  `cargo run`.
- **Everything in the repository is in English**: identifiers, comments, user-facing messages,
  command names, documentation and commit messages.
- Keep the code simple and readable rather than advanced-idiomatic. No premature abstraction.
- When a compiler error is pasted, explain it before fixing it.
- Before handing back: `cargo build`, `cargo clippy` and `cargo fmt` without warnings.
- Think Mac **and** Linux for every choice (paths, audio, drag and drop, packaging).

## Current stack

- Rust edition 2024, a Cargo workspace: the `samplr` library and command line (root), and the
  `samplr-app` interface (Tauri 2, `src-tauri/`, see "Step 7 in progress").
- `symphonia` 0.5 (feature `all`): file info and decoding. **Keep 0.5**, it is the version
  `rodio` uses; moving to 0.6 would compile two versions.
- `rodio` 0.23 (feature `symphonia-all`): audio playback. API:
  `DeviceSinkBuilder::open_default_sink()`, `Player::connect_new(mixer)`,
  `Decoder::try_from(file)`.
- `walkdir`: recursive folder walking.
- `rusqlite` 0.41 (feature `bundled`: SQLite compiled with the project, no system library to
  install). No `usize` in queries: rusqlite cannot convert it, use `u32` or `i64`.
- `dirs`: the system's standard data folder.
- `dunce`: use `dunce::canonicalize`, never `Path::canonicalize`. Identical on Mac and Linux;
  on Windows it avoids `\\?\C:\…` paths, unreadable and different from the ones `notify`
  reports. Likewise, never hard-code `/` as a separator (`std::path::MAIN_SEPARATOR`).
- `notify-debouncer-full` 0.7 (re-exports `notify` 8): folder watching. Groups the events of a
  single file for 1 s, and matches both ends of a rename (`RenameMode::Both`), including on Mac
  where FSEvents sends them separately.
- `clap` 4 (feature `derive`): subcommands are described by the `enum Command` in `main.rs`;
  the `///` comments become the `samplr help` output.
- `trash` 5: "Move file(s) to the trash" (freedesktop trash on Linux, the Finder's on Mac).
- `ebur128` 0.1: a sample's loudness (LUFS, peak), measured with its waveform.
- rusqlite's `functions` feature: `unicode_lower`, the search's case folding (see `samplr search`).
- Interface (`src-tauri/Cargo.toml`): `tauri` 2, `tauri-plugin-drag` (native drag into a DAW,
  patched in `vendor/drag`), `tauri-plugin-dialog` (folder chooser, questions),
  `tauri-plugin-window-state` (window size and position kept between launches).
- **ffmpeg**, an external program rather than a crate: sample conversion (`src/convert.rs`).
  Found through `SAMPLR_FFMPEG`, the `PATH`, then the Homebrew and MacPorts folders; to bundle
  as a Tauri sidecar at packaging (step 11).

- Debug builds compile the **dependencies with optimisations** (`[profile.dev.package."*"]` in
  the root `Cargo.toml`): unoptimised, symphonia decoded an MP3 about 30 times slower (waveform
  of a 54 s MP3: 2.3 s, 0.24 s with this setting, 0.07 s in release). Costs a few minutes once,
  after a `cargo clean` or a dependency update; our own code still rebuilds in about a second.

Linux system dependencies: `build-essential pkg-config libasound2-dev` (ALSA).

On the Mac, the project is opened from the Linux machine's SMB share
(`/Volumes/nico/Sites/projets/sample browser`). The share does not support the file locking
Cargo needs ("could not create session directory lock file"): the Mac's `~/.cargo/config.toml`
therefore sends the build to `/Users/nico/cargo-target` (`build.target-dir`), which also avoids
mixing macOS and Linux binaries in `target/`. Do not put this setting in the project's
`.cargo/config.toml`: the path does not exist on Linux.

## Current state (step 6 done)

`src/main.rs` holds the commands, `src/lib.rs` the shared code (file reading and indexing),
`src/db.rs` the database access, `src/waveform.rs` the waveform computation and drawing,
`src/watch.rs` the folder following, `src/analysis.rs` the tempo, key and loop or one-shot of a
sample, `src/acid.rs` the ACID chunk of WAV files, `src/info.rs` a file's own metadata (Info
panel), `src/convert.rs` the conversion through ffmpeg. The interface's Rust side is in
`src-tauri/src/main.rs` (commands, audio thread) and `src-tauri/src/library.rs` (live following,
background waveforms); its page in `ui/` (`index.html`, `main.js`, `style.css`, `theme.js`):

- `samplr scan <folder>`: lists the audio files (wav, aif, aiff, flac, mp3, ogg, m4a) with
  length, sample rate, bits and channels, and saves them to the database (absolute path; a new
  scan updates without duplicates and keeps the `id`, hence the tags). Unreadable files are
  counted, without crashing. AppleDouble `._*` files left by macOS are ignored, and so is
  anything hidden below a scanned folder (`is_hidden_below`: a name starting with a dot, file or
  folder), by the scan, the startup catch-up and the live following alike: the trash of a disk
  scanned at its root (`.Trash-1000` on Linux, `.Trashes` on Mac) would otherwise bring back
  every sample moved to the trash, and the disk's system folders (`.Spotlight-V100`,
  `.fseventsd`) are not samples. A file moved into a hidden folder is forgotten like a deleted
  one, what an older samplr imported from there is forgotten at the next catch-up, and "Move
  file(s) to folder…" refuses a hidden folder. A scanned folder may itself be hidden: only what
  lies below it counts.
- `samplr list`: shows the samples in the database, with their tags between brackets. Output
  goes through `writeln!` rather than `println!`: `samplr list | head` stops without error when
  `head` closes the pipe ("broken pipe"), and `scan` runs to the end even if its output is cut
  off.
- `samplr tag <file> <tag>...` / `samplr untag <file> <tag>...`: adds or removes tags. `tag`
  reads the file again and adds it to the database if it was not there.
- `samplr tags`: the tags and how many samples carry each.
- `samplr fav <file>...` / `samplr unfav <file>...`: marks samples as favorites or no longer
  (`fav` adds a file to the database if needed, like `tag`). `samplr search --fav` keeps only
  the favorites; `list` and `search` show a ★ before a favorite.
- `samplr folders`: the scanned folders and how many samples each holds.
  `samplr remove-folder <folder>`: the folder is no longer followed and its samples are
  **hidden**, not deleted: they leave the list (search, `list`, `tags`, waveform computing) but
  keep their row and their tags, which come back if the folder is scanned again (`db::save`
  sets `hidden` back to 0). Their waveforms are deleted. A sample still inside another scanned
  folder stays shown. The audio files are never touched.
- `samplr watch`: first catches up with the changes made while it was not running (vanished
  samples removed, new ones added), then follows the scanned folders live until Ctrl+C. A
  rename or a move, of a file or a folder, keeps the `id` hence the tags. A scanned folder
  inside another one is watched only once.
- `samplr search [tag]... [--path <text>]`: samples carrying **all** the given tags, whose path
  contains the text (case-insensitive, accents included, `_` taken literally). SQLite's `lower`
  only knows ASCII: the search compares through `unicode_lower`, Rust's `to_lowercase`
  registered in `db::open` (rusqlite feature `functions`).
- `samplr play <file>`: plays a file on the default output.
- `samplr wave <file> [--width N]`: draws the waveform in the terminal, computing and caching
  it first if needed.
- `samplr waveforms`: computes every missing or stale waveform (about 0.6 s for 10 min of audio
  in release). `scan` does not compute waveforms: decoding every file would make it much slower
  on a big library.

Database: `samplr.db` in the system data folder (`~/.local/share/samplr` on Linux,
`~/Library/Application Support/samplr` on Mac). The `SAMPLR_DB` variable points to another
database, for experiments. Tables `files` (with `hidden`, see `remove-folder`, `favorite`, `bpm`,
`key` and `looped` set by hand, `acid_beats`, `acid_root` and `acid_loop` from the file's ACID
chunk, `codec` as symphonia names it: "pcm_s24le", "flac", "mp3"…), `tags`, `file_tags`, `folders`
(the folders given to `scan`, followed by `watch`), `waveforms`, `collections` and
`collection_files` (samples gathered by hand; a sample forgotten by the database leaves its
collections, a hidden one stays in them and comes back with its folder), `colors` (the palette:
name, `#rrggbb`, position; filled with 10 colours by the migration to version 4). `tags` and
`collections` have a `color_id` (`ON DELETE SET NULL`: deleting a colour leaves them without
colour). A tag deleted because no sample carries it any more loses its colour. Schema changes go
through `db::migrate`: SQLite keeps the schema version (`PRAGMA user_version`, 9 so far), each
step runs once, under `BEGIN IMMEDIATE` so that the app and a command opening the database
together do not both run it. A new database goes through every step too (the `CREATE TABLE`s are
version 0): add a column with a migration step, not in the `CREATE TABLE`, and raise
`SCHEMA_VERSION` with it: an up-to-date database is opened without taking the write lock,
otherwise every command, even a search, waited behind any process writing and failed after 5 s
("database is locked"). For the same reason `index` (scan, startup catch-up, a folder appearing)
reads the files outside any transaction and saves them by batches of 200, each in a short
transaction: the lock is never held for the whole walk, which can take minutes on a slow disk or a
network share. A waveform is 1024 (min, max) peaks over all channels, stored as signed bytes (2
KB), with the file's size and modification date: if either has changed, the waveform is stale and
computed again. The waveform's row also holds the sample's loudness (`lufs`, `peak_db`, with
`measured` = 1), measured from the sound itself during the same decoding (`waveform::compute`,
`ebur128` crate: integrated loudness per EBU R128, the broadcast standard, and the sample peak;
checked against ffmpeg's `ebur128` filter: same values). No LUFS under 0.4 s, the standard's block
(2865 of the 3303 samples of the Linux library have one, all a peak). Waveforms saved before
(`measured` = 0) are stale: the background thread measured the whole library again once (276 s in
a debug build, 15 MB at most), telling the page every 5 s so that the LUFS column fills in. Peaks
are built on the fly by merging buckets two by two, so memory use does not depend on the file
length, which is not always known in advance (some MP3s). Tags are stored lowercase, without
spaces; a tag no longer used by any sample is deleted. **Unplugged disk**: if the scanned folder
itself has disappeared, nothing is removed from the database, at startup or live. Otherwise
unplugging an external disk would erase the tags of all its samples.

Scanning and playback have been tested on Linux and on the Mac (files read from the SMB share).

Audio pitfalls met so far:

- `sleep_until_end()` returns before the sound card's buffer is drained: without the 300 ms
  pause that follows in `play`, a very short sample (75 ms kick) is dropped without being
  played.
- An HDMI output suspended by PipeWire takes one to two seconds to wake up and swallows a short
  sample. Not a samplr bug: fix it on the system side, with a WirePlumber rule
  `session.suspend-timeout-seconds = 0` on the output concerned.
- Mac, file on the SMB share: an "audio stream error: A buffer underrun or overrun occurred"
  appeared once (not reproduced in 6 attempts). Likely cause: `Decoder::try_from(file)` reads
  the file chunk by chunk during playback, hence over the network. Edge case (the share is only
  used for testing): playback keeps streaming from disk. Loading the file into memory would
  delay the start of long files, which would hurt quick previewing from one sample to the next.
- Mac, HDMI output (DELL U2415 monitor): macOS has no volume control for an HDMI output, sound
  goes out at full level. SoundSource adds a software volume, but applies it **per app**: a
  program launched from the command line (`samplr`, `afplay`) is attached to the terminal that
  launched it. Launched from iTerm2, `samplr` shows up under "iTerm2" in SoundSource and
  follows its volume: not a `cpal`/rodio problem. The loud sound came from launches in another
  terminal, whose SoundSource entry was not attenuated. A samplr volume control
  (`Player::set_volume`) is still useful for the interface.
- Apple `.m4r` ringtones (AAC in MP4, e.g.
  `/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/Ringtones/Opening.m4r`)
  are not decoded by symphonia 0.5: length found, channels unknown, no sample. The same sound
  re-encoded by `afconvert` (AAC or WAV) plays fine: AAC is supported, the structure of these
  files is the problem.
- When the decoder produces no sample, `play` reports it ("no sound decoded"): it checks this
  on a throwaway decoder before opening the audio output.

## Roadmap

1. ✅ Scan a folder and list the samples.
2. ✅ Preview: `play` tested on Linux and on the Mac. Volume: in the interface only (step 7).
3. ✅ **SQLite** database (`rusqlite`): files, tags and link tables. Tags stay in the database,
   never in the audio files.
4. ✅ CLI commands to tag and search, with `clap`.
5. ✅ Folder watching (`samplr watch`, and the interface while it is open), tested on Linux
   only. Remaining: test it on the Mac (renames). Removing a folder: `samplr remove-folder` and
   the interface's Folders window.
6. ✅ Waveform computation and cache in the database. The interface computes the missing ones in
   the background; `samplr watch` does not.
7. ⏳ Graphical interface with **Tauri** (Rust backend + web interface), keyboard preview. See
   "Step 7 in progress" below.
8. ⏳ Drag and drop of samples into the DAW: written with `tauri-plugin-drag` (native drag and
   drop on Mac, Linux and Windows). Tested on Linux into Nautilus, a terminal and Bitwig; to
   test on the Mac. **Linux pitfalls**: the `drag` crate (under the plugin) is patched in
   `vendor/drag` (`[patch.crates-io]` in the root `Cargo.toml`): it did not escape file URIs (a
   `#` or a space broke the path) and disconnected its handlers before the receiver asked for
   the file (upstream issue crabnebula-dev/drag-rs#84). Under Wayland, GTK 3 answers only the
   first request for the file and the drop arrives empty: the app therefore sets
   `GDK_BACKEND=x11` at startup (XWayland) unless the variable is already set. The page cancels
   `dragstart`, otherwise WebKit starts its own drag alongside the native one. A drag starts
   after the mouse moves 5 pixels with the button held, so a click still selects and plays; the
   file is offered as a copy (`mode: "copy"`), never moved; the drag image is the sample's name
   drawn on a canvas. The plugin's JavaScript API is `window.__TAURI__.drag` (global API
   script, no npm), allowed by `drag:default` in `src-tauri/capabilities/default.json`.
9. ✅ Favorites ✅ (a `favorite` column, not a tag: a decision taken). Collections ✅: samples
   gathered by hand (not saved searches: a decision taken), with multiple selection and
   dragging rows onto a collection. Tempo and key ✅: from the file name and the length, or set
   by hand.
10. ✅ Settings panel ("Settings…" button), in four sections each titled in small capitals and
    separated by a line (Appearance: theme; Confirmations; Colour palette; Keyboard shortcuts),
    scrolling inside the window when the palette is long: theme, confirmations (one checkbox, on
    by default: ask before deleting a collection, removing a folder or deleting a colour, used or
    not, the question saying what uses it; removing samples from a collection is never confirmed,
    moving files to the trash always is), colour palette. The window closes with its × at the top
    right, Esc, or a press outside it (only the press counts, so that selecting text and releasing
    outside does not close it); the Folders window too (`closeOnPressOutside`, `.dialog-head`).
    `confirmAction` in `main.js` asks, or not; the choice is kept in `localStorage`
    (`askBeforeDeleting`). The shortcuts are listed there, not in the footer (`shortcutList`,
    drawn as `<kbd>`); on a Mac they show the Mac's keys (`shortcut()`: ⌘ for Ctrl, ⇧ for Shift, ⌫
    for Del), and so do the tooltips naming a shortcut: they are written for Linux in the code,
    which listens to `metaKey` as well as `ctrlKey`.
11. Packaging: embed ffmpeg, signed and notarised `.app` / `.dmg` on Mac, AppImage or `.deb` 
    on Linux.

## Step 7 in progress (7a to 7d done, tested on Linux)

- Tauri 2 stable (2.12.x at the time of writing; 3.x is only in alpha).
- Fedora packages Tauri needs (installed on the Linux machine):
  `webkit2gtk4.1-devel libsoup3-devel gtk3-devel javascriptcoregtk4.1-devel libappindicator-gtk3-devel`.
  Already present: `openssl-devel`, `librsvg2-devel`. Node and npm are available through nvm.
- Layout: the crate is a library (`src/lib.rs`: `db`, `waveform`, `SampleInfo`, `read_info`,
  `index`; done) plus the CLI binary (`src/main.rs`, `use samplr::…`). The Tauri app lives in
  `src-tauri/`, in a Cargo workspace declared in the root `Cargo.toml` (one `Cargo.lock`, one
  `target/`).
- Preview is played on the Rust side with rodio, not by the web view: every format symphonia
  reads is supported (WebKitGTK lacks some, e.g. AIFF), and an output kept open for the whole
  session avoids the short-sample problems listed above.
- Planned sub-steps:
    - 7a: library + CLI + workspace, then a first Tauri window.
    - 7b: sample list, search, keyboard navigation, preview played by Rust.
    - 7c: waveform display from the peaks cached in the database.
    - 7d: adding and removing tags from the interface.
- Decisions taken: plain HTML, CSS and JavaScript in `ui/`, no build tool and no Node
  (`withGlobalTauri` exposes the API as `window.__TAURI__`); waveform drawn on a `<canvas>`
  from the cached peaks, no wavesurfer.js.
- Run the interface with `cargo run -p samplr-app`; `cargo run` alone still runs the CLI. No
  Tauri CLI needed for development. The icon's source is `src-tauri/icons/icon.svg` (five amber
  bars of a waveform on a dark rounded square); Tauri takes PNG icons, not SVG, so `icon.png`
  (512 × 512) is exported from it:
  `cd src-tauri/icons && inkscape icon.svg --export-type=png --export-filename=icon.png -w 512 -h 512`.
  Packaging (step 11) will also need `.icns` and `.ico`, generated from the same source.
- 7b: one search box (each word must be in the path or in one of the tags; a `#tag` word
  requires that exact tag), the list of results, ↑ ↓ to move and play, Enter to replay, Esc to
  stop, a click plays. An audio thread opens the output once for the whole session and gets the
  sounds to play through a channel; each sound gets a new `rodio::Player`, dropping the
  previous one cuts it. The file is opened by the `play` command, so an unreadable file is
  reported to the page. Every query of the list goes through `fetchSamples` (`search` for a new
  search, `refresh` after a change): each request is numbered and an answer overtaken by a newer
  request is dropped, otherwise a slow answer coming last replaced the list with results for
  text no longer in the box.
- 7c: the selected sample's waveform is drawn on a `<canvas>` above the list, at the screen's
  real resolution. `samplr::peaks` (in `src/lib.rs`, shared with `samplr wave`) returns the
  cached peaks or computes and caches them. A late answer for a sample no longer selected is
  ignored.
- 7d: a tag bar under the waveform shows the selected sample's tags, each with a × to remove
  it. Ctrl+T (Cmd+T on Mac) moves to the tag box; there, Enter adds the tags typed (separated
  by spaces), Backspace on an empty box removes the last tag shown in the bar (`commonTags`:
  with several samples selected, the last tag they all carry, never one of the current row
  only), Esc goes back to the search box.
  The `add_tags` and `remove_tag` commands return the sample's tags after the change.
- Window: the page itself never scrolls (`overflow: hidden` on `body`); the list, the collections
  and the windows scroll inside, each on its own. The header wraps its buttons onto a second line
  in a narrow window (`flex-wrap`, search box at least 12rem): unwrapped they needed about 1020
  px, and below that the whole page scrolled sideways with the last buttons out of the window. The
  window's size, position and maximised state are saved when the app closes and restored before
  the window shows, by the official `tauri-plugin-window-state` (in
  `~/.config/fr.dataetic.samplr/.window-state.json` on Linux, the system's config folder on Mac);
  it also copes with a screen unplugged meanwhile.
- Preview controls: Play, Stop (same as Esc), Space (`playOrPause`: a loop pauses and resumes
  where it was, `pause` command → `AudioRequest::Pause`, `Player::pause` / `play`, the playhead
  frozen meanwhile; a one-shot plays again from the start, like a pad; nothing playing, the
  selected sample plays), L (switches the Play loop box, as a click on it would), the Play loop
  and Autoplay checkboxes and a volume slider in the header. Autoplay off: a click on a row
  selects it without playing (to tag or drag it in silence), and stops the sound playing if it was
  another sample's; the arrow keys, Enter and Play still play; remembered in `localStorage`
  (`autoplay`). "Play loop" checked: every sample played loops, until the box is unchecked (Stop
  does not change it); remembered in `localStorage` (`loop`). A sound cannot change mode while
  playing: toggling the box starts the selected sample again in the new mode, or stops a sound no
  longer selected. A loop uses rodio's `build_looped`, which seeks back to the start of the file
  at each end instead of keeping the sound in memory; the audio thread receives either decoder as
  a `Box<dyn Source + Send>`. The slider's position is squared before reaching rodio (`set_volume`
  command, 0.0 to 1.0), so that it acts evenly over its whole travel (a linear slider barely
  changes the loudness in its top half); it is remembered in the web view's `localStorage`. The
  audio thread receives `AudioRequest::Play`, `Stop` or `Volume`, and applies the volume to the
  sound playing and to the next ones. It waits for requests at most 100 ms at a time, to notice
  when the sound has played to its end: it then sends the page a `playback-ended` event with the
  number the page gave to that sound (`play` command's `id`), and the page ignores the end of a
  sound already replaced by another one. The audio thread is started in Tauri's `setup`, to get
  the `AppHandle` it sends events with.
- Playhead: a line over the waveform (a `<div>` moved in CSS, not a redraw of the canvas). The
  audio thread sends a `playback-position` event every 100 ms (`Player::get_pos`); in between,
  the page moves the line on the clock (`requestAnimationFrame`). In loop mode the position is
  taken modulo the sample's length. No line when the length is unknown (some MP3s).
- A click on the waveform plays the sample from that point (`play` command's `start`, applied
  with `Player::try_seek` right after `append`), in the current mode (once or loop). Ignored
  when the length is unknown.
- Adding a folder: the "+ Folder" button opens the system's folder chooser
  (`tauri-plugin-dialog`, JavaScript API `window.__TAURI__.dialog`, allowed by
  `dialog:default`), then the `add_folder` command scans it like `samplr scan`: both call
  `samplr::scan` (in `src/lib.rs`). Progress bar: `samplr::scan` first counts the audio files
  (`count_audio_files`, a walk that reads no file), then `index` calls a callback after each
  audio file, readable or not; `add_folder` turns it into `scan-progress` events (`done`,
  `total`), at most one every 100 ms, shown as a `<progress>` in the status line. Until the
  first event the bar shows an activity without a value. The folder is saved and watched
  before the scan starts (like at startup, watching comes before catching up): a file added or
  renamed during a long scan, in a part already gone through, is seen by the watcher.
- Live following: while the app is open, the scanned folders are followed like `samplr watch`
  (catch-up at startup, then live changes; renames keep the tags; an unplugged disk is left
  alone). The code is shared: `src/watch.rs` in the library (`roots`, `new_watcher`,
  `catch_up`, `handle_event`), which reports what changed through a callback; `samplr watch`
  prints it, the app (`src-tauri/src/library.rs`) sends the page a `library-changed` event
  after each batch of changes, and the page searches again, keeping the selected sample and the
  sound playing. If the selected sample left the library (folder removed, file deleted or
  renamed) and is the one playing, its sound stops; a sound only left out by the current search
  keeps playing. `Library` (in `State`) holds the watcher and the watched folders, so that
  `add_folder` watches a new folder at once (`update_watched`). Running `samplr watch` at the
  same time works but is useless: both apply the same changes. A failure never stops the
  following of the other folders: a folder that cannot be watched (a subfolder without read
  permission makes inotify refuse the whole folder; too many folders for the system) is left out
  and tried again at the next `update_watched`, a folder that cannot be caught up with is still
  followed live, a change that cannot be applied is skipped. Each is kept as a problem
  (`Library::problem`), and the page shows them in the red error box: it asks for them
  (`library_problems`, which empties the list) at startup, after adding or removing a folder,
  and on the `library-problem` event.
- Folders window ("Folders…" button, a `<dialog>`): the scanned folders with their sample
  count, "(not found)" when missing from disk, and a Remove button each, confirmed through
  `window.__TAURI__.dialog.ask` (the plugin's `message` command, allowed by `dialog:default`).
  Commands `folders` and `remove_folder` (which also stops watching the folder, through
  `Library::update_watched`). While the window is open, the list's keys are off.
- Dropping on a collection: rows dragged out of the list (the native drag of `startDrag`) or
  files from a file manager, dropped on a collection of the left column, are added to it. The
  page learns of the drag only through Tauri's drag-and-drop events
  (`getCurrentWebview().onDragDropEvent`: enter, over, drop, leave), whose position is in
  physical pixels (divided by `devicePixelRatio` for `elementFromPoint`); each collection's
  `<li>` carries `data-id`. The collection under the cursor is outlined. `add_to_collection`
  leaves out the paths not in the library (files from elsewhere, folders) and reports how many
  (`unknown`).
- Virtual list: only the rows on screen, plus 10 above and below, are in the table (`drawRows`),
  between two empty `spacer` rows that give it its full height; scrolling draws again only when
  the screen leaves the rows drawn, at most once per frame. Rows have a fixed height (26 px in
  CSS, measured on the first row shown); the current row is outlined with inner shadows, not
  borders, which could add a pixel. `rowElement(index)` gives a sample's row or `null` when off
  screen; `ensureVisible(index)` scrolls just enough to show the row. The header is a table of its
  own above the list (`#list-head`), outside its scrolling, not a `position: sticky` one:
  WebKitGTK drew the text of the rows passing under a sticky header over its top, depending on the
  scroll position (`z-index`, a forced layer, a sticky `thead`, separate borders and `contain` did
  not help; checked in WebKitGTK's MiniBrowser). The rows' table gets a copy of the header's
  `<colgroup>` at startup, every width is set on both, and the header follows the list sideways
  (`scrollLeft`). The rows' events (click, star, ×, drag, right-click) are listened to once on the
  `<tbody>`, each row carrying `data-index`. Measured on 3287 samples (Chromium, fake Tauri API):
  a search went from 195 ms to 16 ms, building the list from 136 ms to 2 ms. Anything that changes
  a row must go through `rowElement`, never `tbody.children[index]`.
- Collections column: for each sample, the collections holding it, each after its colour's dot.
  `search` gives each sample's collection `id`s (`SampleInfo::collections`); the page takes
  names and colours from its `collections` list, so a colour change needs no new search. After
  adding to, removing from or deleting a collection, the list is refreshed
  (`refresh({ stopIfGone: false })`, selection and scroll kept).
- Sorting: a click on a column header (★, Name, Length, BPM, Key, Collections, Tags, Folder) sorts
  the list, a second click reverses it (▲/▼ on the header). Done in the page (`sortSamples`), kept
  across searches and refreshes; with no column chosen, the database's order (by path). Natural
  order, case ignored (`Intl.Collator` with `numeric`): "kick 2" before "kick 10". Missing values
  (unknown length, no tag) stay last whichever the direction; equal values fall back to the path.
- Column widths: the table has a fixed layout (`table-layout: fixed`), its widths come from the
  `<col>` elements. Columns in this order: ★, Name, Length, BPM, Key, Loop, LUFS, Ch (channels),
  Codec, Collections, Tags, Folder (last: the longest and least read), then the × in a collection.
  A handle on the right edge of every header but Folder changes the width by dragging (pointer
  capture), a double-click brings the default back (Tags: 200 px); Folder takes the rest, at least
  120 px (`minFolderWidth`), ★ and × have fixed widths. Tags is narrowed like any other column,
  its tags then ending with an ellipsis (it used to take the rest and never be narrower than every
  sample's tags, measured over the whole library at each change; dropped). When the chosen widths
  plus Folder's minimum do not fit, the table grows wider than the list, which scrolls sideways.
  `applyColumnWidths` sets the tables' `width` in pixels (the larger of the list's width and what
  the columns need), recomputed on window resize and folding: WebKit ignores `min-width` on a
  fixed-layout table. The virtual list's spacer rows span exactly the columns shown, counted on
  the header: one more made a phantom column that took a share of the free width. Widths are never
  shrunk to fit. Widths kept in `localStorage` (`columnWidths`; a width stored for Folder by an
  older samplr is dropped). A click on a handle does not sort. Outside a collection the × column's
  `<col>` is taken out of the tables: a zero-width `<col>` is treated as automatic by WebKit and
  would share the free width. Text too long for its cell ends with an ellipsis. One row in two is
  slightly darker (`striped`, `--stripe` mixed from the text and background colours), after the
  sample's place in the list rather than `nth-child`: the virtual list's rows alternate
  differently as it scrolls. Every row has a background of its own (`--bg` otherwise): WebKitGTK
  smooths the text of a row without one differently, and it looked bold next to a striped one.
- Visual feedback: after adding samples to a collection (menu or drop), their rows and the
  collection flash; after adding tags, the rows and the tag bar (`flash` in `main.js`, a CSS
  animation from the waveform colour back to the element's own background).
- Multiple selection: click selects one row, Ctrl/Cmd+click adds or takes out a row, Shift+click
  and Shift+↑↓ select from the anchor (the last row clicked without Shift) to the row, Ctrl/Cmd+A
  selects every sample of the list, rows off screen included (`selectAll`, no sound; in a text box
  it still selects the text). No text of the page can be selected (`user-select: none` on `body`,
  prefixed too for WebKitGTK; text boxes and error messages excepted): a Shift+click selected the
  page's text from the last place clicked. The page keeps `selected` (the current row: played,
  waveform shown, outlined when several are selected), `selectedPaths` (paths rather than rows, so
  that a refresh keeps the selection) and `anchor`. Tags (the tag bar shows the tags common to the
  selection), Ctrl+D (all become favorites, unless all already are), the collection commands and
  Del act on the whole selection; the ★ of a row and its × in a collection act on that row only.
  Dragging a selected row drags the whole selection to the DAW (the drag image says how many);
  dragging a row outside the selection drags that row alone. A right-click inside the selection
  keeps it. `add_tags`, `remove_tag` and `set_favorite` take a list of paths and work in one
  transaction.
- Favorites: a ★/☆ column at the start of each row (a click toggles it without selecting nor
  playing), Ctrl+D / Cmd+D toggles the selected sample, and a "★ Favorites" button filters the
  list (`search` command's `favorites`). Command `set_favorite`. With the filter on, a sample
  no longer a favorite stays in the list until the next search.
- Collections: the column on the left lists "All samples" then the collections, each with its
  number of samples; a click opens one in the list (search and Favorites filter apply within it).
  The form under the list (a name box and a "+" button on one line, Enter creates too)
  creates an empty one. A right-click on a row selects it without playing and opens a menu
  (`#context-menu`, built in the page): add to a collection (those already holding the sample are
  shown with ✓, greyed and not clickable, with a × next to them that takes the selection out:
  `collections_holding` command, which returns the collections holding **all** the given samples;
  a big selection is asked by parts of 10 000, SQLite taking at most 32 766 values per query), add
  to a new collection named in the menu (adding, × and a new collection keep the menu open and
  redraw it with the new state; only a click elsewhere or Esc closes it; the menu waits for
  `collections_holding` before showing, and Esc or a press elsewhere meanwhile cancels it: each
  opening is numbered in `menuOpening`, which closing moves on), and, in an open collection,
  remove from it. The menu also acts on the files themselves (an explicit action, see the design
  decisions): "Rename file…" (one sample: the button turns into a box holding the name without its
  extension, which is kept; `rename_sample`) and "Move file(s) to the trash…" (the selection,
  always asked first, whatever the Settings checkbox says: these are files, not links;
  `trash_samples` → `samplr::trash_sample`: the `trash` crate sends the file to the system's
  trash, freedesktop `~/.local/share/Trash` on Linux, the Finder's on Mac, with its original path
  to put it back; the row is forgotten, so tags, collections and favorite are lost even if the
  file is put back) and "Move file(s) to folder…" (the selection, into a folder picked with the
  system's chooser, which must be inside a scanned folder, otherwise the samples would leave the
  library unfollowed; `move_samples`, stops at the first failure). Both go through
  `samplr::move_sample`: never overwrites, copies then removes between two disks
  (`ErrorKind::CrossesDevices`), and moves the database row with `db::rename`, keeping the `id`
  hence tags, collections, favorite and waveform; the folder watcher then finds the row already
  moved, which changes nothing. The page follows the new paths (`pathsChanged`: selection and
  sound playing kept). Del (Backspace on a Mac keyboard) also removes, when the focus is not in a
  text box, and so does the × at the end of each row (that row's sample, selected or not). After a
  removal the list is refreshed keeping the selection, and the sound keeps playing (`refresh({
  stopIfGone: false })`). Removing is not confirmed (only links go); deleting a collection (×) is.
  Commands `collections`, `create_collection`, `delete_collection`, `add_to_collection`,
  `remove_from_collection`; `search` takes the open `collection`. No CLI command yet. The column
  folds into a thin strip (‹ / › button, Ctrl+B / Cmd+B), remembered in `localStorage` like the
  volume. Its width changes by dragging its right edge (`#collections-resizer`, 120 px to half the
  window, double-click for the default 13rem), remembered in `localStorage` (`collectionsWidth`),
  so that long names can be read; a name too long still ends with an ellipsis and shows whole in
  its tooltip.
- Tags box: under the collections, in the same column (`#tag-list`), every tag with its number of
  samples shown (`tags` command, `db::list_tags`); the two lists share the column's height, each
  scrolling on its own, and fold with it. A handle between them (`#lists-resizer`, the column
  handles' design lying down, with a fine `--border` line in its middle separating the two boxes)
  sets the collections' height, the tags taking the rest, neither below 48 px; a double-click
  shares it equally again; kept in `localStorage` (`collectionsHeight`); the collections' list may
  still shrink in a smaller window (`flex: 0 1 …`). Sorted by name (natural order, as the list) or
  by count, most used first then by name, with the "Name" / "Count" buttons of its head; the
  choice is kept in `localStorage` (`tagSort`). A click puts `#tag` in the search box, or takes it
  out, so that tags can be combined; the tags of the search, clicked or typed in any case, are
  marked. A right-click opens the colour menu, as in the tag bar. Reloaded after tags change
  (`showTags`), after `refresh` (changes on disk, folder removed, files trashed) and at startup;
  redrawn after colours change and after each search. Collections and tags without colour get an
  empty dot, so that every name is aligned ("All samples" has none).
- Show in Finder / file manager (row menu, the selection; `reveal`): `open -R` on Mac; on Linux
  the freedesktop `org.freedesktop.FileManager1.ShowItems` D-Bus method through `gdbus` (part of
  GLib, which Tauri needs anyway; Nautilus, Dolphin, Nemo answer it), with the files as `file://`
  URIs percent-encoded byte by byte (`file_uri`, unit-tested), and the first file's folder opened
  with `xdg-open` when nothing answers. No crate added: `tauri-plugin-opener` would do the same.
- Conversion: "Convert file(s)…" in the row menu (the selection; greyed when ffmpeg is not
  found, `ffmpeg_available`) opens a window (`#convert`) asking the format (WAV, AIFF, FLAC,
  MP3, OGG Vorbis), its settings (bit depth 16, 24 or as the original for the lossless ones, MP3
  bit rate, OGG quality) and "Mix down to mono", offered only when a selected sample is stereo
  (or of unknown channels); in a mixed selection the mono ones are only converted. The choices
  are remembered in `localStorage` (`convertOptions`). `convert_samples` →
  `samplr::convert_sample` writes a copy next to each original, which is kept (same name, the
  new extension, " mono" when mixed down; never overwrites), and adds it to the library with
  the original's tags and collections. The encoding is done by **ffmpeg**, run as a separate
  program (`src/convert.rs`: `SAMPLR_FFMPEG`, else `ffmpeg` in the `PATH`, then in
  `/opt/homebrew/bin`, `/usr/local/bin` and `/opt/local/bin`, since an app opened from the
  Finder or the Dock gets a minimal `PATH` without them, unlike one launched from a terminal;
  written to a hidden `.name.ext.part` the folder watcher ignores, then renamed): every format,
  and no sound held in samplr's memory. A pure-Rust encoder (`hound`, `flacenc`) was tried and
  dropped: two formats only, and streaming FLAC needed care ffmpeg already takes. Packaging
  (step 11) will have to bundle an LGPL, audio-only ffmpeg as a Tauri sidecar (`externalBin`),
  signed with the app on Mac.
- Colours: the Settings window lists the palette (colour picker, name, ✎, ×; "+ Add colour"); a
  name is read only until its ✎ is pressed (`editColorName`: Enter or leaving the box saves,
  Esc puts it back without closing the window), a new colour's name opens ready to edit; each
  change saved at once (`change` events); a new colour takes the hue farthest from the
  palette's and a name after its hue and lightness (`colorName`: "Lime", "Dark teal", "Grey
  2"); a name still automatic follows the colour when it changes (`isAutomaticName`), one typed
  by hand stays; deleting a colour asks first (unless turned off in Settings), saying how many
  tags and collections use it. A right-click on a tag of the tag bar or on a collection opens a
  menu of the palette's colours plus "No colour" (`openColorMenu`, in `#context-menu`). Shown
  as chips in the tag bar and in the list's Tags column (text black or white after the colour's
  brightness, `textOn`) and a dot before collections (left column and the row menu). No tint on
  whole rows: tried, but a sample with tags of several colours showed only the first one, which
  misled (a decision taken). The page keeps `palette` and `tagColorIds` (tag name → colour id).
  Commands `colors`, `add_color`, `update_color`, `color_usage`, `delete_color`, `tag_colors`,
  `set_tag_color`, `set_collection_color`; `collections` gives each one's `color_id`. While the
  Settings window is open, the list's keys are off.
- Background waveforms: a thread of `Library` computes every missing or stale waveform
  (`samplr::update_waveform`, shared with `samplr waveforms`) after the startup catch-up, after
  each batch of changes on disk and after `add_folder`. Wake-ups that come during a pass are
  merged into one more pass. Unreadable files are tried again at each pass (nothing is cached
  for them). The page's `waveform` command still computes a waveform not ready yet; both may
  compute the same one, harmlessly (`INSERT OR REPLACE`).
- Themes: every colour of `ui/style.css` comes from variables of `:root`; a theme redefines
  them under `:root[data-theme="…"]`. Six palettes of samplr's own, each light and dark, named
  after music genres (Soul, Synthwave, House, Jungle, Dub, Techno; `data-theme` keys
  `soul-light`, `synthwave-dark`…); "System" (no `data-theme`) is Soul, light or dark after the
  computer's setting (`light-dark()`). Three colours per palette: `--wave` the main one
  (waveform, stars, current row, handles, drag image), `--selected` the second (selection, open
  collection, tag chips), `--signal` the complementary one (active buttons, flash after an
  addition, drop target); the complementary is yellow in every palette but Soul (indigo).
  Neutrals `--bg`, `--fg`, `--surface`, `--header`, `--border`, `--muted` are tinted towards
  each palette's hue; `--hover`, `--signal-soft`, `--button` and `--button-hover` are derived
  with `color-mix`. The labelled buttons (header, Create, Remove of the Folders window, + Add
  colour) are flat and compact: no border, a fill of `--wave` mixed into the background, rather
  than the system's raised buttons (big and grey under GTK). Contrast checked against WCAG:
  text 4.5 on the background and on the selection, muted text, `--wave` and `--signal` 3 (the
  light themes' yellows are deepened towards ochre for that). The choice is made in Settings,
  kept in `localStorage` (`theme`) and applied before the first paint by `ui/theme.js`, loaded
  in `<head>`; `color-scheme` per theme makes native controls follow. The palettes were chosen
  on a comparison page: https://claude.ai/artifact/Cn6ULagC4uJHfUiKfrpmq8
- Spacing: every padding, margin and gap of `ui/style.css` comes from the `--space-1` …
  `--space-12` variables of `:root` (named after tenths of a rem: 0.1, 0.2, 0.3, 0.4, 0.6, 0.8,
  1.2 rem); only `0` and the playhead's centring `-1px` stay raw. Use the scale for any new
  spacing rather than a new value. The tag bar's chips and its input are siblings in `#tagbar`,
  so that one gap separates them all; only the chips are replaced when the selection changes
  (moving the input would take the focus from it).
- Tempo and key (`src/analysis.rs`, unit-tested: `cargo test -p samplr --lib analysis`): from the
  file name, in the two conventions of sample packs ("Djembe 100BPM D#m", "YCE_120_Gm_Horns": a
  bare number right before a key), else from the file's ACID chunk (`src/acid.rs`, unit-tested:
  its length in beats × 60 ÷ the length, shown without "≈", tooltip "From the file"; its root note
  for the key, a note without major or minor), else the tempo from the length when a loop's length
  gives a whole tempo (60 × beats ÷ seconds for 4 to 128 beats, between 70 and 180, nearest 120).
  A lone letter is a key only next to a tempo ("Kick_A" is not in A) or with a quality ("Cmaj",
  "F#m"); the note's letter must be upper case. Keys in sharps ("A#m" for "Bbm"). Worked out at
  each search (`db::search` fills `SampleInfo::bpm`, `bpm_source` "name", "file", "length" or
  "manual", `key`), not stored: a renamed file needs nothing; the ACID chunk is read with the file
  (`read_info`, chunk headers only, never the sound) and kept in `files`. The chunk's own tempo
  field is not used: 120 in 461 of the 466 files of the Linux library having one, whatever the
  sample, while beats ÷ length gave the tempo of the name 348 times out of 348, and the root note
  the key of the name 348 times out of 348; on that library it gives an exact tempo to 118 samples
  more (113 of them were estimates from the length). Splice's samples carry no such chunk: their
  tempo, key and tags stay in the Splice app. symphonia's tags were measured too: only 18 files
  (music MP3 and FLAC, no WAV) have any. Set by hand: a double-click on a BPM or Key cell edits
  it, for the whole selection when the row is in it (Enter or leaving the box saves, Esc cancels;
  a value refused after Enter keeps the box open, outlined in `--error` red, the reason in its
  tooltip and the status line; refused on leaving the box, the old value comes back; empty goes
  back to automatic, `-` means none, to silence a wrong guess). A click on these cells selects the
  row without playing it, and the second click of the double-click does nothing (`event.detail`);
  elsewhere quick clicks on a row still retrigger the sound, like a pad. Stored in `files.bpm` and
  `files.key` (migration 5): NULL automatic, 0 and '' none; commands `set_tempo` (20 to 400) and
  `set_key` (any spelling, kept in sharps). The edit box has no border nor vertical padding: a
  taller row would break the virtual list's fixed height. Columns BPM ("≈ 100" muted for an
  estimate) and Key, sortable and resizable; search filters `bpm:120`, `bpm:118-124` (either
  order, 20 to 400 like a tempo set by hand: `TEMPOS`), `key:Am` (any spelling: `key:bbm`),
  applied in the `search` command after the SQL query. On the 3287 samples of the Linux library:
  tempo from the name for 51 %, from the length for 11 %, key for 30 %. **No analysis of the sound
  itself** (a decision taken, measured): stratum-dsp 1.0 (pure Rust) found the right tempo for 34
  % of the 795 samples whose name gives it (35 % under 4 s, 31 % for 10 to 30 s), the right key
  for 13 % of 974 (mostly a fifth off), 158 ms and up to 600 MB per file for two minutes of sound;
  the length method was right 87 % of the time when it answers. Its announced 87.7 % is on full DJ
  tracks, which the library hardly has; worth measuring again on long files only, with its
  confidence ≥ 0.5 (80 % right on the few labelled ones over 10 s). Never decode a whole file into
  memory for an analysis: a 3-hour recording took the machine's memory.
- Loop or one-shot (`analysis::looped`, unit-tested): set by hand first (`files.looped`), else the
  ACID chunk's flag, else the words of the file name then of its folders from the deepest up, the
  first that says it winning (`loop_from_words`: "loop", "loops"; "one shot", "oneshot", "hit",
  "hits", "fx", "sfx", "fill", "fills", so that `… Loops/HITS/Kick/kick.wav` is a one-shot), else
  a loop when a tempo is known (a guess, greyed). Worked out at each search (`SampleInfo::looped`,
  `loop_source`), shown in the Loop column as ✓ or —, sortable, searched with `loop:yes` /
  `loop:no`. A double-click switches it for the selection (`switchLoop`, `set_loop`); a value
  equal to the one worked out is not kept as set by hand, so switching twice goes back to
  automatic. On the Linux library: 466 decided by the chunk, 2295 by the words, 540 guessed from
  the tempo; effects and fills were taken for loops when their length happened to give a whole
  tempo, hence their words.
- Info panel (`#info`, on the right of the list, folds with its › button or Ctrl+I, remembered in
  `localStorage`, `infoFolded`): the current sample's name, folder, format ("WAV · 24 bit · 44.1
  kHz · stereo"), length, size and modification date, tempo, key and loop with where they come
  from, its tags, then under "In the file" what the file says of itself (`file_info` command →
  `samplr::info::file_tags`, read when the current sample changes, not stored: `renderInfo` is
  called by `renderTags`, the file read again only for another sample). WAV: the `LIST/INFO`
  chunk, read by samplr since symphonia does not (title, album = the pack, artist, album artist,
  genre, comment, copyright, software, source…; 2092 of the 3198 WAV files of the Linux library
  have one, Future Loops and Splice among them). Other formats: symphonia's tags (ID3 of MP3,
  Vorbis comments of FLAC and OGG). symphonia 0.5 does not read the ID3 chunk of an AIFF (Splice's
  KSHMR loops carry only a URL there): not read yet.
- Errors: `showError` in `main.js` shows them in a red box above the status line (`#error-box`,
  `--error` the same red in every theme, closed by its × or after 10 s, not while the pointer is
  on it; its text can be selected and copied) and in the status line; every failure reported to
  the user goes through it, never through the status line alone, which nobody notices.
- Rust functions called from the page are marked `#[tauri::command]` in `src-tauri/src/main.rs`
  and listed in `generate_handler!`. They return `Result<T, String>`: the error must be
  serialisable to reach JavaScript, `Box<dyn Error>` is not.

## Known limitations

- Path case: on Mac (by default) and on Windows, `Kick.wav` and `kick.wav` are the same file,
  but the database compares paths as case-sensitive text. A rename that only changes case, or a
  path typed with another case in `tag`, may not find the sample. Needs a design decision.
- Unicode normalisation: on Mac, file names written by HFS+ (and some apps) are decomposed (NFD:
  "e" then a combining accent), while text typed in the search box is composed (NFC). A search
  for "été" may then miss "été" in such a name. Fix: normalise both to NFC
  (`unicode-normalization` crate) in `unicode_lower`. Not done yet: to check on the Mac first.

## Design decisions

- **Never move audio files without an explicit user action**: DAW projects point to their
  paths. Prefer virtual collections (tags, favourites, smart folders).
- App data in the system's standard folder (`~/.local/share` on Linux,
  `~/Library/Application Support` on Mac), through the `dirs` crate.
