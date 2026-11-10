// samplr's graphical interface: a window showing the web page of ui/, which
// calls the Rust functions marked `#[tauri::command]` below.

// On Windows, a release build starts without opening a console window
// next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod library;
mod stretch;

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rodio::{Decoder, Source};
use samplr::waveform::Peak;
use samplr::{SampleInfo, analysis, db};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use library::Library;

/// What the audio thread is asked to do.
enum AudioRequest {
    /// A sound played once or in a loop: two different decoder types, hence
    /// the `Box<dyn Source>`, which holds either. `id` is chosen by the page
    /// and sent back when the sound ends.
    Play {
        sound: Box<dyn Source + Send>,
        id: u32,
        /// Where to start, in seconds from the beginning.
        start: f64,
        /// Sent with the sound, so that it starts already shaped.
        shape: Playback,
    },
    Stop,
    /// Pauses (`true`) or resumes the sound playing, where it was.
    Pause(bool),
    /// 0.0 is silence, 1.0 the file's own level.
    Volume(rodio::Float),
    Shape(Playback),
}

/// How the preview is played: `speed` as a tape (2.0 an octave up and twice
/// as fast), then a tempo factor and a transposition in semitones changed
/// one without the other (see `stretch`).
#[derive(Clone, Copy)]
struct Playback {
    speed: f32,
    tempo: f32,
    semitones: f32,
}

impl Playback {
    /// Within what the page offers, whatever it sends.
    fn new(speed: f32, tempo: f32, semitones: f32) -> Self {
        Playback {
            speed: speed.clamp(0.25, 4.0),
            tempo: tempo.clamp(0.25, 4.0),
            semitones: semitones.clamp(-24.0, 24.0),
        }
    }
}

/// Shared by every command through `State`: the way to talk to the audio thread.
struct Audio {
    requests: Sender<AudioRequest>,
}

/// Samples matching the search box. Each word must be found in the path or
/// in one of the tags (`kick` finds `Kick_01.wav` and the samples tagged
/// `kick` or `kicks`); a word starting with `#` is a tag the sample must
/// carry exactly. `favorites`: only the favorites. `collection`: only the
/// samples of that collection (`null` from the page: the whole library).
/// `(async)` runs the command outside the main thread: the window stays
/// responsive while the database answers.
/// The error travels to the page as text: `Box<dyn Error>` cannot be turned
/// into JSON, a `String` can.
#[tauri::command(async)]
fn search(
    query: String,
    favorites: bool,
    collection: Option<i64>,
) -> Result<Vec<SampleInfo>, String> {
    let mut tags = Vec::new();
    let mut words = Vec::new();
    let mut tempos = None;
    let mut key = None;
    let mut looped = None;
    let mut sound_type = None;
    for word in query.split_whitespace() {
        // `bpm:` and `key:` filters; one still being typed and not valid
        // yet (`bpm:1`, `key:`) filters nothing.
        let lower = word.to_lowercase();
        if let Some(value) = lower.strip_prefix("bpm:") {
            tempos = tempo_range(value).or(tempos);
            continue;
        }
        if let Some(value) = word.get(4..).filter(|_| lower.starts_with("key:")) {
            key = analysis::normalize_key(value).or(key);
            continue;
        }
        if let Some(value) = lower.strip_prefix("loop:") {
            looped = match value {
                "yes" | "y" | "true" | "1" => Some(true),
                "no" | "n" | "false" | "0" => Some(false),
                // Still being typed: no filter.
                _ => looped,
            };
            continue;
        }
        if let Some(value) = lower.strip_prefix("type:") {
            if samplr::sound_type::is_known(value) {
                sound_type = Some(value.to_string());
            }
            continue;
        }
        match word.strip_prefix('#') {
            Some("") => {} // a lone `#` while the user is still typing
            Some(tag) => tags.push(tag.to_string()),
            None => words.push(word),
        }
    }

    let db = db::open().map_err(|e| e.to_string())?;
    let mut samples =
        db::search(&db, &tags, &words, "", favorites, collection).map_err(|e| e.to_string())?;
    // Tempo and key are worked out from the name and the length, not stored:
    // they are filtered here rather than in SQL.
    if let Some(tempos) = tempos {
        samples.retain(|sample| sample.bpm.is_some_and(|bpm| tempos.contains(&bpm)));
    }
    if let Some(key) = key {
        samples.retain(|sample| sample.key.as_ref() == Some(&key));
    }
    if let Some(looped) = looped {
        samples.retain(|sample| sample.looped == looped);
    }
    // `type:drums` keeps the whole family, `type:kick` the kicks only,
    // `type:electric` electric guitars and basses.
    if let Some(wanted) = sound_type {
        samples.retain(|sample| {
            sample
                .sound_type
                .as_deref()
                .is_some_and(|found| samplr::sound_type::matches(found, &wanted))
        });
    }
    Ok(samples)
}

/// `120` or `118-124`, within the tempos samples have.
fn tempo_range(value: &str) -> Option<std::ops::RangeInclusive<u32>> {
    let (low, high) = value.split_once('-').unwrap_or((value, value));
    let (low, high): (u32, u32) = (low.parse().ok()?, high.parse().ok()?);
    // In either order: `bpm:150-120` is 120 to 150.
    let (low, high) = (low.min(high), low.max(high));
    // A number still being typed (`bpm:1` on the way to `bpm:120`) filters
    // nothing rather than everything.
    (TEMPOS.contains(&low) && TEMPOS.contains(&high)).then_some(low..=high)
}

/// The tempos a sample can be given by hand, hence searched for.
const TEMPOS: std::ops::RangeInclusive<u32> = 20..=400;

/// The palette, in its order.
#[tauri::command(async)]
fn colors() -> Result<Vec<db::Color>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::colors(&db).map_err(|e| e.to_string())
}

/// Adds a colour at the end of the palette and returns its `id`.
#[tauri::command(async)]
fn add_color(name: String, hex: String) -> Result<i64, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::add_color(&db, &name, &hex).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn update_color(id: i64, name: String, hex: String) -> Result<(), String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::update_color(&db, id, &name, &hex).map_err(|e| e.to_string())
}

/// How many tags and collections use a colour: `[tags, collections]`.
#[tauri::command(async)]
fn color_usage(id: i64) -> Result<(u32, u32), String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::color_usage(&db, id).map_err(|e| e.to_string())
}

/// Deletes a colour; its tags and collections are left without colour.
#[tauri::command(async)]
fn delete_color(id: i64) -> Result<(), String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::delete_color(&db, id).map_err(|e| e.to_string())
}

/// Every tag with the number of samples shown carrying it: `[name, count]`
/// pairs, by name.
#[tauri::command(async)]
fn tags() -> Result<Vec<(String, u32)>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::list_tags(&db).map_err(|e| e.to_string())
}

/// The tags that have a colour: `[name, colour id]` pairs.
#[tauri::command(async)]
fn tag_colors() -> Result<Vec<(String, i64)>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::tag_colors(&db).map_err(|e| e.to_string())
}

/// Gives a tag a colour, or none (`null` from the page).
#[tauri::command(async)]
fn set_tag_color(tag: String, color: Option<i64>) -> Result<(), String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::set_tag_color(&db, &tag, color).map_err(|e| e.to_string())
}

/// Gives a collection a colour, or none (`null` from the page).
#[tauri::command(async)]
fn set_collection_color(collection: i64, color: Option<i64>) -> Result<(), String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::set_collection_color(&db, collection, color).map_err(|e| e.to_string())
}

/// The collections, sorted by name, with their number of samples.
#[tauri::command(async)]
fn collections() -> Result<Vec<db::Collection>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::collections(&db).map_err(|e| e.to_string())
}

/// Creates an empty collection and returns its `id`.
#[tauri::command(async)]
fn create_collection(name: String) -> Result<i64, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::create_collection(&db, &name).map_err(|e| e.to_string())
}

/// Deletes a collection. The samples are not touched.
#[tauri::command(async)]
fn delete_collection(collection: i64) -> Result<(), String> {
    let db = db::open().map_err(|e| e.to_string())?;
    db::delete_collection(&db, collection).map_err(|e| e.to_string())
}

/// What `add_to_collection` reports to the page.
#[derive(Serialize)]
struct AddedToCollection {
    /// Samples added (those already in the collection are not counted).
    added: usize,
    /// Paths left out because they are not in the library: files dropped
    /// from a file manager, folders.
    unknown: usize,
}

/// Adds samples to a collection.
#[tauri::command(async)]
fn add_to_collection(collection: i64, paths: Vec<PathBuf>) -> Result<AddedToCollection, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let mut file_ids = Vec::new();
    for path in &paths {
        if let Some(file_id) = db::file_id(&db, path).map_err(|e| e.to_string())? {
            file_ids.push(file_id);
        }
    }
    let added = db::add_to_collection(&db, collection, &file_ids).map_err(|e| e.to_string())?;
    Ok(AddedToCollection {
        added,
        unknown: paths.len() - file_ids.len(),
    })
}

/// The `id`s of the collections that already hold all these samples.
#[tauri::command(async)]
fn collections_holding(paths: Vec<PathBuf>) -> Result<Vec<i64>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let file_ids = file_ids(&db, &paths)?;
    db::collections_holding(&db, &file_ids).map_err(|e| e.to_string())
}

/// Takes samples out of a collection; they stay in the library. Returns the
/// number removed.
#[tauri::command(async)]
fn remove_from_collection(collection: i64, paths: Vec<PathBuf>) -> Result<usize, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let file_ids = file_ids(&db, &paths)?;
    db::remove_from_collection(&db, collection, &file_ids).map_err(|e| e.to_string())
}

/// Marks samples as favorites, or no longer.
#[tauri::command(async)]
fn set_favorite(paths: Vec<PathBuf>, favorite: bool) -> Result<(), String> {
    let mut db = db::open().map_err(|e| e.to_string())?;
    // All the samples or none, and a single write to disk.
    let transaction = db.transaction().map_err(|e| e.to_string())?;
    for file_id in file_ids(&transaction, &paths)? {
        db::set_favorite(&transaction, file_id, favorite).map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())
}

/// Sets the samples' tempo by hand. `value`: a tempo, empty or `"-"` for
/// none, `"auto"` to go back to the one worked out (name, file, sound…).
/// Emptying the box means none: going back to automatic gave a detected
/// tempo, which looked like the box refusing to be emptied.
#[tauri::command(async)]
fn set_tempo(paths: Vec<PathBuf>, value: String) -> Result<(), String> {
    let value = value.trim();
    let bpm = match value {
        "auto" => None,
        "" | "-" => Some(0),
        _ => match value.parse::<u32>() {
            Ok(bpm) if TEMPOS.contains(&bpm) => Some(bpm),
            _ => {
                let (low, high) = (TEMPOS.start(), TEMPOS.end());
                return Err(format!("“{value}” is not a tempo between {low} and {high}"));
            }
        },
    };
    let mut db = db::open().map_err(|e| e.to_string())?;
    let transaction = db.transaction().map_err(|e| e.to_string())?;
    for file_id in file_ids(&transaction, &paths)? {
        db::set_tempo(&transaction, file_id, bpm).map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())
}

/// What `detect_tempo` did: how many samples got a tempo, and how many gave
/// none (too short, too long, unreadable).
#[derive(Serialize)]
struct Detected {
    detected: u32,
    failed: u32,
}

/// "Detect BPM": works out the samples' tempo by the method chosen
/// ("name", "file", "length" or "sound") and keeps it over any other, even
/// one typed by hand. "sound" decodes each file whole: a few tenths of a
/// second per loop.
#[tauri::command(async)]
fn detect_tempo(paths: Vec<PathBuf>, method: String) -> Result<Detected, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let mut result = Detected {
        detected: 0,
        failed: 0,
    };
    for (path, file_id) in paths.iter().zip(file_ids(&db, &paths)?) {
        match samplr::detect_tempo(&db, file_id, path, &method) {
            Ok(Some(bpm)) => {
                db::set_detected_tempo(&db, file_id, bpm, &method).map_err(|e| e.to_string())?;
                result.detected += 1;
            }
            _ => result.failed += 1,
        }
    }
    Ok(result)
}

/// What each method of "Detect BPM" but the sound gives for a sample.
#[tauri::command(async)]
fn tempo_methods(path: PathBuf) -> Result<samplr::TempoMethods, String> {
    samplr::tempo_methods(&path).map_err(|e| e.to_string())
}

/// Sets samples as loops (`true`) or one-shots by hand; `None` goes back to
/// the value worked out.
#[tauri::command(async)]
fn set_loop(paths: Vec<PathBuf>, looped: Option<bool>) -> Result<(), String> {
    let mut db = db::open().map_err(|e| e.to_string())?;
    let transaction = db.transaction().map_err(|e| e.to_string())?;
    for file_id in file_ids(&transaction, &paths)? {
        db::set_loop(&transaction, file_id, looped).map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())
}

/// Sets the samples' kind of sound by hand. `value`: `family` or
/// `family/subtype` of `sound_type::FAMILIES`, `"-"` for none, empty to go
/// back to the one worked out. A type equal to the one worked out is not
/// kept as set by hand, so that the sample follows a later rename.
#[tauri::command(async)]
fn set_sound_type(paths: Vec<PathBuf>, value: String) -> Result<(), String> {
    let chosen = match value.trim() {
        "" => None,
        "-" => Some(""),
        text if samplr::sound_type::parse(text).is_some() => Some(text),
        text => return Err(format!("“{text}” is not a type of sound")),
    };
    let mut db = db::open().map_err(|e| e.to_string())?;
    let transaction = db.transaction().map_err(|e| e.to_string())?;
    for (path, file_id) in paths.iter().zip(file_ids(&transaction, &paths)?) {
        let worked_out = samplr::sound_type::from_path(path).unwrap_or_default();
        let kept = chosen.filter(|chosen| *chosen != worked_out);
        db::set_sound_type(&transaction, file_id, kept).map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())
}

/// A family and its subtypes, for the page's type picker.
#[derive(Serialize)]
struct SoundFamily {
    id: &'static str,
    label: &'static str,
    subtypes: Vec<SoundSubtype>,
}

#[derive(Serialize)]
struct SoundSubtype {
    id: &'static str,
    label: &'static str,
}

#[tauri::command]
fn sound_types() -> Vec<SoundFamily> {
    samplr::sound_type::FAMILIES
        .iter()
        .map(|family| SoundFamily {
            id: family.id,
            label: family.label,
            subtypes: family
                .subtypes
                .iter()
                .map(|subtype| SoundSubtype {
                    id: subtype.id,
                    label: subtype.label,
                })
                .collect(),
        })
        .collect()
}

/// Sets the samples' key by hand, in any spelling ("Bbm", "a#min"). `value`:
/// empty or `"-"` for none, `"auto"` to go back to the one read in the name
/// or the file, as for the tempo.
#[tauri::command(async)]
fn set_key(paths: Vec<PathBuf>, value: String) -> Result<(), String> {
    let value = value.trim();
    let key = match value {
        "auto" => None,
        "" | "-" => Some(String::new()),
        _ => match analysis::normalize_key(value) {
            Some(key) => Some(key),
            None => return Err(format!("“{value}” is not a key (e.g. Am, F#, Bbm)")),
        },
    };
    let mut db = db::open().map_err(|e| e.to_string())?;
    let transaction = db.transaction().map_err(|e| e.to_string())?;
    for file_id in file_ids(&transaction, &paths)? {
        db::set_key(&transaction, file_id, key.as_deref()).map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())
}

/// Renames a sample's file, in its folder, keeping its extension: `name` is
/// the new name without it. Returns the new path.
#[tauri::command(async)]
fn rename_sample(path: PathBuf, name: String) -> Result<PathBuf, String> {
    let name = name.trim();
    // A name, not a path: no separator, nothing that would leave the folder.
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(format!("“{name}” is not a file name"));
    }
    let mut file_name = name.to_string();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        file_name = format!("{file_name}.{extension}");
    }
    let new = path.with_file_name(file_name);
    if new == path {
        return Ok(path);
    }
    let db = db::open().map_err(|e| e.to_string())?;
    file_id(&db, &path)?;
    samplr::move_sample(&db, &path, &new).map_err(|e| e.to_string())?;
    Ok(new)
}

/// Moves samples into `folder`, which must be inside a scanned folder: moved
/// elsewhere, they would leave the library without anything to follow them.
/// Stops at the first failure (a file of that name already there); the
/// samples moved before it stay moved. Returns the new paths, in order.
#[tauri::command(async)]
fn move_samples(paths: Vec<PathBuf>, folder: PathBuf) -> Result<Vec<PathBuf>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let folder = dunce::canonicalize(&folder).map_err(|e| e.to_string())?;
    let scanned = db::folders(&db).map_err(|e| e.to_string())?;
    if !scanned.iter().any(|root| folder.starts_with(root)) {
        return Err(format!(
            "{} is not in a scanned folder: add it with + Folder first",
            folder.display()
        ));
    }
    if samplr::is_hidden_below(&folder, &scanned) {
        return Err(format!(
            "{} is a hidden folder, whose files are not part of the library",
            folder.display()
        ));
    }
    let mut moved = Vec::new();
    for path in &paths {
        file_id(&db, path)?;
        let name = path.file_name().ok_or("no file name")?;
        let new = folder.join(name);
        if new != *path {
            samplr::move_sample(&db, path, &new)
                .map_err(|e| format!("{e} ({} sample(s) moved before)", moved.len()))?;
        }
        moved.push(new);
    }
    Ok(moved)
}

/// Sends samples' files to the system's trash. Stops at the first failure;
/// those trashed before it stay in the trash. Returns how many were trashed.
#[tauri::command(async)]
fn trash_samples(paths: Vec<PathBuf>) -> Result<u32, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let mut trashed = 0;
    for path in &paths {
        file_id(&db, path)?;
        samplr::trash_sample(&db, path).map_err(|e| {
            format!(
                "{}: {e} ({trashed} sample(s) trashed before)",
                path.display()
            )
        })?;
        trashed += 1;
    }
    Ok(trashed)
}

/// True when ffmpeg, which converts samples, is installed.
#[tauri::command]
fn ffmpeg_available() -> bool {
    samplr::convert::ffmpeg().is_some()
}

/// Converts samples after `options` (format, depth or bit rate, mono), each
/// next to its original, which is kept. Stops at the first failure; the
/// copies made before it stay. Returns the new paths, in order.
#[tauri::command(async)]
fn convert_samples(
    paths: Vec<PathBuf>,
    options: samplr::convert::Options,
) -> Result<Vec<PathBuf>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let mut converted = Vec::new();
    for path in &paths {
        let copy = samplr::convert_sample(&db, path, &options).map_err(|e| {
            format!(
                "{}: {e} ({} sample(s) converted before)",
                path.display(),
                converted.len()
            )
        })?;
        converted.push(copy);
    }
    Ok(converted)
}

/// What went wrong while following the folders since the last call (a
/// folder that cannot be watched, a change on disk not applied), for the
/// page to show. Emptied by the call.
#[tauri::command]
fn library_problems(library: State<Library>) -> Vec<String> {
    library.take_problems()
}

/// Shows samples' files in the system's file manager, selected: the Finder
/// on Mac (`open -R`); on Linux the file manager answering the freedesktop
/// `FileManager1` D-Bus interface (Nautilus, Dolphin, Nemo…), through
/// `gdbus`, which comes with GLib, already needed by Tauri; failing that,
/// the folder of the first file is opened.
#[tauri::command(async)]
fn reveal(paths: Vec<PathBuf>) -> Result<(), String> {
    let first = paths.first().ok_or("no file to show")?;
    if let Some(missing) = paths.iter().find(|path| !path.exists()) {
        return Err(format!("{} is not on disk", missing.display()));
    }
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg("-R")
            .args(&paths)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("the Finder could not show {}", first.display()));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let uris: Vec<String> = paths
            .iter()
            .map(|path| format!("'{}'", file_uri(path)))
            .collect();
        let shown = std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.FileManager1",
                "--object-path",
                "/org/freedesktop/FileManager1",
                "--method",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("[{}]", uris.join(", ")),
                "",
            ])
            .output()
            .is_ok_and(|output| output.status.success());
        if !shown {
            let folder = first.parent().unwrap_or(first);
            let status = std::process::Command::new("xdg-open")
                .arg(folder)
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("no file manager could show {}", folder.display()));
            }
        }
    }
    Ok(())
}

/// `file:///home/me/kick%20%231.wav`: every byte but letters, digits, `-._~`
/// and `/` percent-encoded, so that a space, a `#` or a quote in a name
/// cannot break the URI nor the D-Bus argument holding it.
#[cfg(not(target_os = "macos"))]
fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// What the Info panel shows of a file besides what the list knows: its
/// size, its modification date and its own metadata (`samplr::info`).
#[derive(Serialize)]
struct FileInfo {
    size: u64,
    /// Seconds since 1970, `None` when the system does not say.
    modified: Option<u64>,
    tags: Vec<(String, String)>,
}

#[tauri::command(async)]
fn file_info(path: PathBuf) -> Result<FileInfo, String> {
    let metadata = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs());
    Ok(FileInfo {
        size: metadata.len(),
        modified,
        tags: samplr::info::file_tags(&path),
    })
}

/// The `playback-position` event: where the sound `id` is, in seconds of the
/// file whatever the speed (a loop keeps counting from its first start).
#[derive(Serialize, Clone)]
struct Position {
    id: u32,
    seconds: f64,
}

/// A scanned folder, as the `folders` command describes it.
#[derive(Serialize)]
struct Folder {
    path: PathBuf,
    /// Samples shown from this folder.
    samples: u32,
    /// `false` when the folder is not on disk (unplugged disk, deleted).
    found: bool,
}

/// The scanned folders, in alphabetical order.
#[tauri::command(async)]
fn folders() -> Result<Vec<Folder>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let mut list = Vec::new();
    for path in db::folders(&db).map_err(|e| e.to_string())? {
        list.push(Folder {
            samples: db::count_under(&db, &path).map_err(|e| e.to_string())?,
            found: path.is_dir(),
            path,
        });
    }
    Ok(list)
}

/// Removes a folder from the library, like `samplr remove-folder`: no longer
/// followed, its samples hidden but kept with their tags. Returns the number
/// of samples hidden.
#[tauri::command(async)]
fn remove_folder(folder: PathBuf, library: State<Library>) -> Result<usize, String> {
    let mut db = db::open().map_err(|e| e.to_string())?;
    let hidden = samplr::remove_folder(&mut db, &folder).map_err(|e| e.to_string())?;
    if let Err(error) = library.update_watched() {
        eprintln!("Watched folders not updated: {error}");
    }
    Ok(hidden)
}

/// The `scan-progress` event: audio files gone through, out of `total`.
#[derive(Serialize, Clone)]
struct ScanProgress {
    done: usize,
    total: usize,
}

/// What `add_folder` reports to the page.
#[derive(Serialize)]
struct Added {
    samples: usize,
    unreadable: usize,
}

/// Adds a folder of samples, like `samplr scan`, and follows it from then
/// on. A big folder takes a while: `(async)` keeps the window responsive
/// meanwhile, and the page receives `scan-progress` events with the number of
/// audio files gone through so far.
#[tauri::command(async)]
fn add_folder(folder: PathBuf, library: State<Library>, app: AppHandle) -> Result<Added, String> {
    let mut db = db::open().map_err(|e| e.to_string())?;
    // Watched before it is scanned, like at startup: a file added or renamed
    // in a part the scan has already gone through is seen by the watcher. The
    // folder must be in the database for `update_watched` to watch it.
    let canonical = dunce::canonicalize(&folder).map_err(|e| e.to_string())?;
    if !canonical.is_dir() {
        return Err(format!("\"{}\" is not a folder", folder.display()));
    }
    db::add_folder(&db, &canonical).map_err(|e| e.to_string())?;
    // A folder that cannot be watched is kept as a problem for the page by
    // `update_watched`; the samples are saved all the same.
    if let Err(error) = library.update_watched() {
        eprintln!("{}: not watched: {error}", canonical.display());
    }
    // At most one event every 100 ms: one per file would flood the page.
    let mut last_report = Instant::now();
    let mut progress = |done: usize, total: usize| {
        if last_report.elapsed() >= Duration::from_millis(100) {
            last_report = Instant::now();
            // Fails only if the window is already closed: nothing to tell.
            let _ = app.emit("scan-progress", ScanProgress { done, total });
        }
    };
    let scan = samplr::scan(&mut db, &canonical, &mut progress).map_err(|e| e.to_string())?;
    library.update_waveforms();
    Ok(Added {
        samples: scan.samples.len(),
        unreadable: scan.unreadable,
    })
}

/// The waveform of a sample of the database, computed and cached first if
/// needed (a long file can take a moment). Each peak reaches the page as a
/// `[low, high]` pair.
#[tauri::command(async)]
fn waveform(path: PathBuf) -> Result<Vec<Peak>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let file_id = file_id(&db, &path)?;
    samplr::peaks(&db, file_id, &path).map_err(|e| e.to_string())
}

/// Adds tags to samples and returns, for each sample in the same order, all
/// its tags in alphabetical order.
#[tauri::command(async)]
fn add_tags(paths: Vec<PathBuf>, tags: Vec<String>) -> Result<Vec<Vec<String>>, String> {
    change_tags(&paths, |db, file_id| {
        db::add_tags(db, file_id, &tags).map(|_| ())
    })
}

/// Removes a tag from samples and returns, for each sample in the same
/// order, the tags it has left.
#[tauri::command(async)]
fn remove_tag(paths: Vec<PathBuf>, tag: String) -> Result<Vec<Vec<String>>, String> {
    let tags = [tag];
    change_tags(&paths, |db, file_id| {
        db::remove_tags(db, file_id, &tags).map(|_| ())
    })
}

/// Applies `change` to each sample, in a single transaction, and returns the
/// tags of each sample afterwards.
fn change_tags(
    paths: &[PathBuf],
    change: impl Fn(&rusqlite::Connection, i64) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<Vec<Vec<String>>, String> {
    let mut db = db::open().map_err(|e| e.to_string())?;
    let transaction = db.transaction().map_err(|e| e.to_string())?;
    let mut all_tags = Vec::new();
    for file_id in file_ids(&transaction, paths)? {
        change(&transaction, file_id).map_err(|e| e.to_string())?;
        all_tags.push(db::tags_of(&transaction, file_id).map_err(|e| e.to_string())?);
    }
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(all_tags)
}

/// The `id`s of samples of the database.
fn file_ids(db: &rusqlite::Connection, paths: &[PathBuf]) -> Result<Vec<i64>, String> {
    paths.iter().map(|path| file_id(db, path)).collect()
}

/// The `id` of a sample of the database.
fn file_id(db: &rusqlite::Connection, path: &Path) -> Result<i64, String> {
    db::file_id(db, path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{} is not in the database", path.display()))
}

/// Starts playing a file from `start` seconds, once or in a loop, cutting the
/// one that was playing. When a sound played once reaches its end, the page
/// receives a `playback-ended` event carrying `id`; a loop never ends by
/// itself.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn play(
    path: PathBuf,
    id: u32,
    looping: bool,
    start: f64,
    speed: f32,
    tempo: f32,
    semitones: f32,
    audio: State<Audio>,
) -> Result<(), String> {
    // The file is opened here rather than in the audio thread, so that an
    // unreadable file is reported to the page.
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = File::open(&path).map_err(|e| error(&e))?;
    let sound: Box<dyn Source + Send> = if looping {
        // Goes back to the start of the file at each end, without loading the
        // whole sound into memory. Same settings as `Decoder::try_from(file)`.
        let length = file.metadata().map_err(|e| error(&e))?.len();
        let decoder = Decoder::builder()
            .with_data(BufReader::new(file))
            .with_byte_len(length)
            .with_seekable(true)
            .build_looped()
            .map_err(|e| error(&e))?;
        Box::new(decoder)
    } else {
        Box::new(Decoder::try_from(file).map_err(|e| error(&e))?)
    };
    let shape = Playback::new(speed, tempo, semitones);
    send(
        &audio,
        AudioRequest::Play {
            sound,
            id,
            start,
            shape,
        },
    )
}

#[tauri::command]
fn stop(audio: State<Audio>) -> Result<(), String> {
    send(&audio, AudioRequest::Stop)
}

#[tauri::command]
fn pause(paused: bool, audio: State<Audio>) -> Result<(), String> {
    send(&audio, AudioRequest::Pause(paused))
}

/// Sets the preview volume, from 0.0 (silence) to 1.0 (the file's own level),
/// for the sound playing and the next ones.
#[tauri::command]
fn set_volume(volume: rodio::Float, audio: State<Audio>) -> Result<(), String> {
    send(&audio, AudioRequest::Volume(volume.clamp(0.0, 1.0)))
}

/// Sets how the preview is played, for the sound playing and the next ones:
/// `speed` changes pitch and tempo together, as a tape; `tempo` (a factor)
/// and `semitones` change one without the other.
#[tauri::command]
fn set_playback(speed: f32, tempo: f32, semitones: f32, audio: State<Audio>) -> Result<(), String> {
    send(
        &audio,
        AudioRequest::Shape(Playback::new(speed, tempo, semitones)),
    )
}

fn send(audio: &Audio, request: AudioRequest) -> Result<(), String> {
    // Sending only fails if the audio thread has stopped, which it does when
    // no audio output could be opened.
    audio
        .requests
        .send(request)
        .map_err(|_| "no audio output available".to_string())
}

/// Runs for the whole session: keeps the audio output open and plays what it
/// is asked to. Opening the output once avoids the problems met with `samplr
/// play`, which opens and closes it every time: an output suspended by the
/// system swallows the start of the sound, and closing it too early cuts
/// a short sample.
fn audio_thread(requests: Receiver<AudioRequest>, app: AppHandle) {
    let mut sink = match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => sink,
        Err(error) => {
            eprintln!("Could not open the audio output: {error}");
            return;
        }
    };
    sink.log_on_drop(false);

    // A new `Player` per sound: replacing the old one drops it, and a
    // dropped `Player` stops its sound at once.
    let mut current: Option<rodio::Player> = None;
    // The id of the sound playing, until its end has been reported.
    let mut playing_id: Option<u32> = None;
    let mut volume = 1.0;
    let shape = stretch::Shape::new();
    // Where the sound playing is in its file. `Player::get_pos` counts the
    // time played, which no longer matches the file once the speed changes
    // during the sound: the position is tracked under the speed instead.
    let mut position = Arc::new(Mutex::new(Duration::ZERO));
    loop {
        // Waits for a request, but no more than 100 ms, to report the
        // position of the sound playing and notice its end.
        match requests.recv_timeout(Duration::from_millis(100)) {
            Ok(AudioRequest::Play {
                sound,
                id,
                start,
                shape: playback,
            }) => {
                shape.set(playback.tempo, playback.semitones);
                let player = rodio::Player::connect_new(sink.mixer());
                player.set_volume(volume);
                player.set_speed(playback.speed);
                position = Arc::new(Mutex::new(Duration::ZERO));
                let shared = Arc::clone(&position);
                let tracked = sound
                    .track_position()
                    .periodic_access(Duration::from_millis(20), move |source| {
                        *shared.lock().unwrap() = source.get_pos()
                    });
                player.append(stretch::Stretched::new(
                    Box::new(tracked),
                    Arc::clone(&shape),
                ));
                if start > 0.0 {
                    // The player's seek is in time played: at twice the
                    // speed, a second played is two of the file. A format
                    // that cannot seek just plays from the start.
                    let _ =
                        player.try_seek(Duration::from_secs_f64(start / f64::from(playback.speed)));
                }
                current = Some(player);
                playing_id = Some(id);
            }
            Ok(AudioRequest::Stop) => {
                current = None;
                playing_id = None;
            }
            Ok(AudioRequest::Pause(paused)) => {
                if let Some(player) = &current {
                    if paused {
                        player.pause();
                    } else {
                        player.play();
                    }
                }
            }
            Ok(AudioRequest::Volume(new_volume)) => {
                volume = new_volume;
                if let Some(player) = &current {
                    player.set_volume(volume);
                }
            }
            Ok(AudioRequest::Shape(playback)) => {
                shape.set(playback.tempo, playback.semitones);
                if let Some(player) = &current {
                    player.set_speed(playback.speed);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The app is closing.
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let (Some(player), Some(id)) = (&current, playing_id) else {
            continue;
        };
        // Emitting fails only if the window is already closed: nothing to
        // tell then.
        if player.empty() {
            // The finished `Player` is kept rather than dropped: its last
            // samples may still be on their way to the sound card.
            playing_id = None;
            let _ = app.emit("playback-ended", id);
        } else {
            let seconds = position.lock().unwrap().as_secs_f64();
            let _ = app.emit("playback-position", Position { id, seconds });
        }
    }
}

fn main() {
    // GTK 3 under Wayland answers only the first request for the dragged
    // file's path: the drop reaches the receiver empty. Through XWayland the
    // drag goes through X11, which works, and is what most Linux DAWs use.
    // A `GDK_BACKEND` set by the user still wins.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("GDK_BACKEND").is_none() {
        // SAFETY: no other thread is running yet to read the environment.
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }

    let (requests, receiver) = mpsc::channel();

    tauri::Builder::default()
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_dialog::init())
        // Saves the window's size, position and maximised state when the
        // app closes, and restores them before the window shows.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        // The audio thread starts once the app exists, to get the handle it
        // sends events to the page with. `move` gives it ownership of
        // `receiver`.
        .setup(move |app| {
            let handle = app.handle().clone();
            thread::spawn(move || audio_thread(receiver, handle));
            library::start(app.handle().clone());
            Ok(())
        })
        // Stored by Tauri and handed to every command that asks for
        // `State<Audio>` or `State<Library>`.
        .manage(Audio { requests })
        .manage(Library::new())
        .invoke_handler(tauri::generate_handler![
            search,
            waveform,
            add_tags,
            remove_tag,
            set_favorite,
            set_tempo,
            set_key,
            set_loop,
            detect_tempo,
            tempo_methods,
            set_sound_type,
            sound_types,
            rename_sample,
            move_samples,
            trash_samples,
            ffmpeg_available,
            library_problems,
            reveal,
            file_info,
            convert_samples,
            colors,
            add_color,
            update_color,
            color_usage,
            delete_color,
            tag_colors,
            tags,
            set_tag_color,
            set_collection_color,
            collections,
            create_collection,
            delete_collection,
            add_to_collection,
            collections_holding,
            remove_from_collection,
            play,
            stop,
            pause,
            set_volume,
            set_playback,
            add_folder,
            folders,
            remove_folder
        ])
        .run(tauri::generate_context!())
        .expect("could not start the interface");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn file_uris() {
        assert_eq!(
            file_uri(Path::new("/home/me/Été kick #1 'x'.wav")),
            "file:///home/me/%C3%89t%C3%A9%20kick%20%231%20%27x%27.wav"
        );
    }

    #[test]
    fn tempo_filters() {
        assert_eq!(tempo_range("120"), Some(120..=120));
        assert_eq!(tempo_range("118-124"), Some(118..=124));
        assert_eq!(tempo_range("150-120"), Some(120..=150));
        // Tempos set by hand outside the old 40 to 300.
        assert_eq!(tempo_range("30"), Some(30..=30));
        assert_eq!(tempo_range("350"), Some(350..=350));
        // Still being typed: no filter.
        assert_eq!(tempo_range("1"), None);
        assert_eq!(tempo_range("118-"), None);
        assert_eq!(tempo_range("12-130"), None);
    }
}
