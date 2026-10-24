// samplr's graphical interface: a window showing the web page of ui/, which
// calls the Rust functions marked `#[tauri::command]` below.

// On Windows, a release build starts without opening a console window
// next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use rodio::Decoder;
use samplr::waveform::Peak;
use samplr::{SampleInfo, db};
use tauri::State;

/// What the audio thread is asked to do: play this sound, or stop (`None`).
/// The decoder reads the file through a buffer (`BufReader`), a few
/// kilobytes at a time rather than byte by byte.
type AudioRequest = Option<Decoder<BufReader<File>>>;

/// Shared by every command through `State`: the way to talk to the audio thread.
struct Audio {
    requests: Sender<AudioRequest>,
}

/// Samples matching the search box. Each word must be found in the path or
/// in one of the tags (`kick` finds `Kick_01.wav` and the samples tagged
/// `kick` or `kicks`); a word starting with `#` is a tag the sample must
/// carry exactly.
/// `(async)` runs the command outside the main thread: the window stays
/// responsive while the database answers.
/// The error travels to the page as text: `Box<dyn Error>` cannot be turned
/// into JSON, a `String` can.
#[tauri::command(async)]
fn search(query: String) -> Result<Vec<SampleInfo>, String> {
    let mut tags = Vec::new();
    let mut words = Vec::new();
    for word in query.split_whitespace() {
        match word.strip_prefix('#') {
            Some("") => {} // a lone `#` while the user is still typing
            Some(tag) => tags.push(tag.to_string()),
            None => words.push(word),
        }
    }

    let db = db::open().map_err(|e| e.to_string())?;
    db::search(&db, &tags, &words, "").map_err(|e| e.to_string())
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

/// Adds tags to a sample and returns all its tags, in alphabetical order.
#[tauri::command(async)]
fn add_tags(path: PathBuf, tags: Vec<String>) -> Result<Vec<String>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let file_id = file_id(&db, &path)?;
    db::add_tags(&db, file_id, &tags).map_err(|e| e.to_string())?;
    db::tags_of(&db, file_id).map_err(|e| e.to_string())
}

/// Removes a tag from a sample and returns the tags it has left.
#[tauri::command(async)]
fn remove_tag(path: PathBuf, tag: String) -> Result<Vec<String>, String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let file_id = file_id(&db, &path)?;
    db::remove_tags(&db, file_id, &[tag]).map_err(|e| e.to_string())?;
    db::tags_of(&db, file_id).map_err(|e| e.to_string())
}

/// The `id` of a sample of the database.
fn file_id(db: &rusqlite::Connection, path: &Path) -> Result<i64, String> {
    db::file_id(db, path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{} is not in the database", path.display()))
}

/// Starts playing a file, cutting the one that was playing.
#[tauri::command(async)]
fn play(path: PathBuf, audio: State<Audio>) -> Result<(), String> {
    // The file is opened here rather than in the audio thread, so that an
    // unreadable file is reported to the page.
    let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decoder = Decoder::try_from(file).map_err(|e| format!("{}: {e}", path.display()))?;
    send(&audio, Some(decoder))
}

#[tauri::command]
fn stop(audio: State<Audio>) -> Result<(), String> {
    send(&audio, None)
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
fn audio_thread(requests: Receiver<AudioRequest>) {
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
    // `for` waits for each request in turn, and ends when the app closes.
    for request in requests {
        current = request.map(|decoder| {
            let player = rodio::Player::connect_new(sink.mixer());
            player.append(decoder);
            player
        });
    }
    drop(current);
}

fn main() {
    let (requests, receiver) = mpsc::channel();
    // `move` gives the closure ownership of `receiver`: the new thread keeps
    // it after `main` has moved on.
    thread::spawn(move || audio_thread(receiver));

    tauri::Builder::default()
        // Stored by Tauri and handed to every command that asks for `State<Audio>`.
        .manage(Audio { requests })
        .invoke_handler(tauri::generate_handler![
            search, waveform, add_tags, remove_tag, play, stop
        ])
        .run(tauri::generate_context!())
        .expect("could not start the interface");
}
