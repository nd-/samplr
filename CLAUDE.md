# samplr: context for Claude

## The project
Desktop audio sample manager for **macOS and Linux**: listen to, tag and organise samples (for musicians, alongside a DAW).

## Who develops
- **Complete Rust beginner**: explain each new concept (ownership, borrowing, `Result`, traits, lifetimes, threads…) when it first appears in the code, in simple words. No upfront theory lessons. Explanations in the conversation stay in French.
- Machines: a Mac M4 (Apple Silicon) and a Linux box. Main development on **Linux with RustRover**.
- Used to Linux keyboard shortcuts. When quoting RustRover shortcuts, give the Linux keymap ones (or both versions).

## How we work
- Move forward in **small steps that compile and run**: each step must be testable with `cargo run`.
- **Everything in the repository is in English**: identifiers, comments, user-facing messages, command names, documentation and commit messages.
- Keep the code simple and readable rather than advanced-idiomatic. No premature abstraction.
- When a compiler error is pasted, explain it before fixing it.
- Before handing back: `cargo build`, `cargo clippy` and `cargo fmt` without warnings.
- Think Mac **and** Linux for every choice (paths, audio, drag and drop, packaging).

## Current stack
- Rust edition 2024, single `samplr` binary (command line for now).
- `symphonia` 0.5 (feature `all`): file info and decoding. **Keep 0.5**, it is the version `rodio` uses; moving to 0.6 would compile two versions.
- `rodio` 0.23 (feature `symphonia-all`): audio playback. API: `DeviceSinkBuilder::open_default_sink()`, `Player::connect_new(mixer)`, `Decoder::try_from(file)`.
- `walkdir`: recursive folder walking.
- `rusqlite` 0.41 (feature `bundled`: SQLite compiled with the project, no system library to install). No `usize` in queries: rusqlite cannot convert it, use `u32` or `i64`.
- `dirs`: the system's standard data folder.
- `dunce`: use `dunce::canonicalize`, never `Path::canonicalize`. Identical on Mac and Linux; on Windows it avoids `\\?\C:\…` paths, unreadable and different from the ones `notify` reports. Likewise, never hard-code `/` as a separator (`std::path::MAIN_SEPARATOR`).
- `notify-debouncer-full` 0.7 (re-exports `notify` 8): folder watching. Groups the events of a single file for 1 s, and matches both ends of a rename (`RenameMode::Both`), including on Mac where FSEvents sends them separately.
- `clap` 4 (feature `derive`): subcommands are described by the `enum Command` in `main.rs`; the `///` comments become the `samplr help` output.

Linux system dependencies: `build-essential pkg-config libasound2-dev` (ALSA).

On the Mac, the project is opened from the Linux machine's SMB share (`/Volumes/nico/Sites/projets/sample browser`). The share does not support the file locking Cargo needs ("could not create session directory lock file"): the Mac's `~/.cargo/config.toml` therefore sends the build to `/Users/nico/cargo-target` (`build.target-dir`), which also avoids mixing macOS and Linux binaries in `target/`. Do not put this setting in the project's `.cargo/config.toml`: the path does not exist on Linux.

## Current state (step 6 done)
`src/main.rs` holds the commands, `src/lib.rs` the shared code (file reading and indexing), `src/db.rs` the database access, `src/waveform.rs` the waveform computation and drawing:
- `samplr scan <folder>`: lists the audio files (wav, aif, aiff, flac, mp3, ogg, m4a) with length, sample rate, bits and channels, and saves them to the database (absolute path; a new scan updates without duplicates and keeps the `id`, hence the tags). Unreadable files are counted, without crashing. AppleDouble `._*` files left by macOS are ignored.
- `samplr list`: shows the samples in the database, with their tags between brackets. Output goes through `writeln!` rather than `println!`: `samplr list | head` stops without error when `head` closes the pipe ("broken pipe"), and `scan` runs to the end even if its output is cut off.
- `samplr tag <file> <tag>...` / `samplr untag <file> <tag>...`: adds or removes tags. `tag` reads the file again and adds it to the database if it was not there.
- `samplr tags`: the tags and how many samples carry each.
- `samplr watch`: first catches up with the changes made while it was not running (vanished samples removed, new ones added), then follows the scanned folders live until Ctrl+C. A rename or a move, of a file or a folder, keeps the `id` hence the tags. A scanned folder inside another one is watched only once.
- `samplr search [tag]... [--path <text>]`: samples carrying **all** the given tags, whose path contains the text (case-insensitive, `_` taken literally).
- `samplr play <file>`: plays a file on the default output.
- `samplr wave <file> [--width N]`: draws the waveform in the terminal, computing and caching it first if needed.
- `samplr waveforms`: computes every missing or stale waveform (about 0.6 s for 10 min of audio in release, about 4 times slower in debug). `scan` does not compute waveforms: decoding every file would make it much slower on a big library.

Database: `samplr.db` in the system data folder (`~/.local/share/samplr` on Linux, `~/Library/Application Support/samplr` on Mac). The `SAMPLR_DB` variable points to another database, for experiments. Tables `files`, `tags`, `file_tags`, `folders` (the folders given to `scan`, followed by `watch`), `waveforms`. A waveform is 1024 (min, max) peaks over all channels, stored as signed bytes (2 KB), with the file's size and modification date: if either has changed, the waveform is stale and computed again. Peaks are built on the fly by merging buckets two by two, so memory use does not depend on the file length, which is not always known in advance (some MP3s). Tags are stored lowercase, without spaces; a tag no longer used by any sample is deleted. **Unplugged disk**: if the scanned folder itself has disappeared, nothing is removed from the database, at startup or live. Otherwise unplugging an external disk would erase the tags of all its samples.

Scanning and playback have been tested on Linux and on the Mac (files read from the SMB share).

Audio pitfalls met so far:
- `sleep_until_end()` returns before the sound card's buffer is drained: without the 300 ms pause that follows in `play`, a very short sample (75 ms kick) is dropped without being played.
- An HDMI output suspended by PipeWire takes one to two seconds to wake up and swallows a short sample. Not a samplr bug: fix it on the system side, with a WirePlumber rule `session.suspend-timeout-seconds = 0` on the output concerned.
- Mac, file on the SMB share: an "audio stream error: A buffer underrun or overrun occurred" appeared once (not reproduced in 6 attempts). Likely cause: `Decoder::try_from(file)` reads the file chunk by chunk during playback, hence over the network. Edge case (the share is only used for testing): playback keeps streaming from disk. Loading the file into memory would delay the start of long files, which would hurt quick previewing from one sample to the next.
- Mac, HDMI output (DELL U2415 monitor): macOS has no volume control for an HDMI output, sound goes out at full level. SoundSource adds a software volume, but applies it **per app**: a program launched from the command line (`samplr`, `afplay`) is attached to the terminal that launched it. Launched from iTerm2, `samplr` shows up under "iTerm2" in SoundSource and follows its volume: not a `cpal`/rodio problem. The loud sound came from launches in another terminal, whose SoundSource entry was not attenuated. A samplr volume control (`Player::set_volume`) is still useful for the interface.
- Apple `.m4r` ringtones (AAC in MP4, e.g. `/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/Ringtones/Opening.m4r`) are not decoded by symphonia 0.5: length found, channels unknown, no sample. The same sound re-encoded by `afconvert` (AAC or WAV) plays fine: AAC is supported, the structure of these files is the problem.
- When the decoder produces no sample, `play` reports it ("no sound decoded"): it checks this on a throwaway decoder before opening the audio output.

## Roadmap
1. ✅ Scan a folder and list the samples.
2. ✅ Preview: `play` tested on Linux and on the Mac (remaining: volume).
3. ✅ **SQLite** database (`rusqlite`): files, tags and link tables. Tags stay in the database, never in the audio files.
4. ✅ CLI commands to tag and search, with `clap`.
5. ✅ Folder watching (`samplr watch`), tested on Linux only. Remaining: test it on the Mac (renames), a command to stop following a scanned folder.
6. ✅ Waveform computation and cache in the database. Remaining: let `watch` compute the waveforms of new files.
7. ⏳ Graphical interface with **Tauri** (Rust backend + web interface), keyboard preview. See "Step 7 in progress" below.
8. Drag and drop of samples into the DAW (test early, the API differs per system).
9. Automatic analysis (BPM, key) and virtual collections.
10. Packaging: signed and notarised `.app` / `.dmg` on Mac, AppImage or `.deb` on Linux.

## Step 7 in progress (7a and 7b done, to be tested)
- Tauri 2 stable (2.12.x at the time of writing; 3.x is only in alpha).
- Fedora packages Tauri needs (installed on the Linux machine): `webkit2gtk4.1-devel libsoup3-devel gtk3-devel javascriptcoregtk4.1-devel libappindicator-gtk3-devel`. Already present: `openssl-devel`, `librsvg2-devel`. Node and npm are available through nvm.
- Layout: the crate is a library (`src/lib.rs`: `db`, `waveform`, `SampleInfo`, `read_info`, `index`; done) plus the CLI binary (`src/main.rs`, `use samplr::…`). The Tauri app lives in `src-tauri/`, in a Cargo workspace declared in the root `Cargo.toml` (one `Cargo.lock`, one `target/`).
- Preview is played on the Rust side with rodio, not by the web view: every format symphonia reads is supported (WebKitGTK lacks some, e.g. AIFF), and an output kept open for the whole session avoids the short-sample problems listed above.
- Planned sub-steps:
  - 7a: library + CLI + workspace, then a first Tauri window.
  - 7b: sample list, search, keyboard navigation, preview played by Rust.
  - 7c: waveform display from the peaks cached in the database.
  - 7d: adding and removing tags from the interface.
- Decisions taken: plain HTML, CSS and JavaScript in `ui/`, no build tool and no Node (`withGlobalTauri` exposes the API as `window.__TAURI__`); waveform drawn on a `<canvas>` from the cached peaks, no wavesurfer.js.
- Run the interface with `cargo run -p samplr-app`; `cargo run` alone still runs the CLI. No Tauri CLI needed for development. `src-tauri/icons/icon.png` is a placeholder icon.
- 7b: one search box (words looked for in the path, `#tag` words as required tags), the list of results, ↑ ↓ to move and play, Enter to replay, Esc to stop, a click plays. An audio thread opens the output once for the whole session and gets the sounds to play through a channel; each sound gets a new `rodio::Player`, dropping the previous one cuts it. The file is opened by the `play` command, so an unreadable file is reported to the page.
- Rust functions called from the page are marked `#[tauri::command]` in `src-tauri/src/main.rs` and listed in `generate_handler!`. They return `Result<T, String>`: the error must be serialisable to reach JavaScript, `Box<dyn Error>` is not.

## Known limitations
- Path case: on Mac (by default) and on Windows, `Kick.wav` and `kick.wav` are the same file, but the database compares paths as case-sensitive text. A rename that only changes case, or a path typed with another case in `tag`, may not find the sample. Needs a design decision.

## Design decisions
- **Never move audio files without an explicit user action**: DAW projects point to their paths. Prefer virtual collections (tags, favourites, smart folders).
- App data in the system's standard folder (`~/.local/share` on Linux, `~/Library/Application Support` on Mac), through the `dirs` crate.
