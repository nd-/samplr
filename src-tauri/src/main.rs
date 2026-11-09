// samplr's graphical interface: a window showing the web page of ui/, which
// calls the Rust functions marked `#[tauri::command]` below.

// On Windows, a release build starts without opening a console window
// next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod library;

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use rodio::{Decoder, Source};
use samplr::waveform::Peak;
use samplr::{SampleInfo, db};
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
    },
    Stop,
    /// 0.0 is silence, 1.0 the file's own level.
    Volume(rodio::Float),
}

/// Shared by every command through `State`: the way to talk to the audio thread.
struct Audio {
    requests: Sender<AudioRequest>,
}

/// Samples matching the search box. Each word must be found in the path or
/// in one of the tags (`kick` finds `Kick_01.wav` and the samples tagged
/// `kick` or `kicks`); a word starting with `#` is a tag the sample must
/// carry exactly. `favorites`: only the favorites.
/// `(async)` runs the command outside the main thread: the window stays
/// responsive while the database answers.
/// The error travels to the page as text: `Box<dyn Error>` cannot be turned
/// into JSON, a `String` can.
#[tauri::command(async)]
fn search(query: String, favorites: bool) -> Result<Vec<SampleInfo>, String> {
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
    db::search(&db, &tags, &words, "", favorites).map_err(|e| e.to_string())
}

/// Marks a sample as a favorite, or no longer.
#[tauri::command(async)]
fn set_favorite(path: PathBuf, favorite: bool) -> Result<(), String> {
    let db = db::open().map_err(|e| e.to_string())?;
    let file_id = file_id(&db, &path)?;
    db::set_favorite(&db, file_id, favorite).map_err(|e| e.to_string())
}

/// The `playback-position` event: where the sound `id` is, in seconds from
/// its start (a loop keeps counting from its first start).
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
    // At most one event every 100 ms: one per file would flood the page.
    let mut last_report = Instant::now();
    let mut progress = |done: usize, total: usize| {
        if last_report.elapsed() >= Duration::from_millis(100) {
            last_report = Instant::now();
            // Fails only if the window is already closed: nothing to tell.
            let _ = app.emit("scan-progress", ScanProgress { done, total });
        }
    };
    let scan = samplr::scan(&mut db, &folder, &mut progress).map_err(|e| e.to_string())?;
    // The samples are saved even if the folder cannot be watched.
    if let Err(error) = library.update_watched() {
        eprintln!("{}: not watched: {error}", scan.folder.display());
    }
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

/// Starts playing a file from `start` seconds, once or in a loop, cutting the
/// one that was playing. When a sound played once reaches its end, the page
/// receives a `playback-ended` event carrying `id`; a loop never ends by
/// itself.
#[tauri::command(async)]
fn play(
    path: PathBuf,
    id: u32,
    looping: bool,
    start: f64,
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
    send(&audio, AudioRequest::Play { sound, id, start })
}

#[tauri::command]
fn stop(audio: State<Audio>) -> Result<(), String> {
    send(&audio, AudioRequest::Stop)
}

/// Sets the preview volume, from 0.0 (silence) to 1.0 (the file's own level),
/// for the sound playing and the next ones.
#[tauri::command]
fn set_volume(volume: rodio::Float, audio: State<Audio>) -> Result<(), String> {
    send(&audio, AudioRequest::Volume(volume.clamp(0.0, 1.0)))
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
    loop {
        // Waits for a request, but no more than 100 ms, to report the
        // position of the sound playing and notice its end.
        match requests.recv_timeout(Duration::from_millis(100)) {
            Ok(AudioRequest::Play { sound, id, start }) => {
                let player = rodio::Player::connect_new(sink.mixer());
                player.set_volume(volume);
                player.append(sound);
                if start > 0.0 {
                    // A format that cannot seek just plays from the start.
                    let _ = player.try_seek(Duration::from_secs_f64(start));
                }
                current = Some(player);
                playing_id = Some(id);
            }
            Ok(AudioRequest::Stop) => {
                current = None;
                playing_id = None;
            }
            Ok(AudioRequest::Volume(new_volume)) => {
                volume = new_volume;
                if let Some(player) = &current {
                    player.set_volume(volume);
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
            let seconds = player.get_pos().as_secs_f64();
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
            play,
            stop,
            set_volume,
            add_folder,
            folders,
            remove_folder
        ])
        .run(tauri::generate_context!())
        .expect("could not start the interface");
}
