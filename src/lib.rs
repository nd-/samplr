// samplr's shared code: what the command line and the graphical interface
// both need (database, waveforms, reading audio files).

pub mod acid;
pub mod analysis;
pub mod convert;
pub mod db;
pub mod info;
pub mod pulse;
pub mod sound_type;
pub mod watch;
pub mod waveform;

use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::{Hint, ProbeResult};
use walkdir::WalkDir;

/// File extensions treated as samples.
pub const AUDIO_EXTENSIONS: [&str; 7] = ["wav", "aif", "aiff", "flac", "mp3", "ogg", "m4a"];

/// The info extracted from each file.
/// `Option<T>` means "maybe a value, maybe nothing": some files do not
/// declare everything.
/// `Serialize` lets Tauri send it to the web page as JSON; the path becomes
/// a string there.
#[derive(Serialize)]
pub struct SampleInfo {
    pub path: PathBuf,
    pub duration_secs: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub bits: Option<u32>,
    /// Empty until the sample is read back from the database.
    pub tags: Vec<String>,
    /// `false` until the sample is read back from the database.
    pub favorite: bool,
    /// The `id`s of the collections holding the sample; empty until it is
    /// read back from the database.
    pub collections: Vec<i64>,
    /// Tempo, from the file name or estimated from the length (see
    /// `analysis`), and where it comes from; `None` when unknown, and until
    /// the sample is read back from the database.
    pub bpm: Option<u32>,
    pub bpm_source: Option<analysis::Source>,
    /// The tempo was chosen with "Detect BPM": stored, it no longer follows
    /// the name or the file.
    pub bpm_fixed: bool,
    /// Key from the file name, in sharps ("A#m"); `None` when unknown.
    pub key: Option<String>,
    /// A loop (`true`) or a one-shot, and where that comes from (see
    /// `analysis::looped`); `false` and `None` until the sample is read back
    /// from the database.
    pub looped: bool,
    pub loop_source: Option<analysis::Source>,
    /// The kind of sound, `family` or `family/subtype` ("drums/kick", see
    /// `sound_type`), and where it comes from: `Name` from the path's words,
    /// `Manual` set by hand.
    pub sound_type: Option<String>,
    pub type_source: Option<analysis::Source>,
    /// Loudness, measured with the waveform: integrated LUFS and peak in
    /// dBFS; `None` until measured, or when it cannot be (too short, silent).
    pub lufs: Option<f64>,
    pub peak_db: Option<f64>,
    /// The codec, as symphonia names it ("pcm_s24le", "flac", "mp3"…).
    pub codec: Option<String>,
    /// The file's ACID chunk, read with the file (`read_info`) and kept in
    /// the database; not sent to the page.
    #[serde(skip)]
    pub acid: Option<acid::Acid>,
}

/// Moves or renames a sample's file to `new` and its database row with it,
/// keeping its `id` (tags, collections, favorite, waveform). Never
/// overwrites a file already at `new`. Between two disks, where the system
/// cannot rename, the file is copied then the original removed.
///
/// The folder watcher will also see the change: it finds nothing recorded
/// at the old path (see `db::rename`) and leaves the moved row as it is.
pub fn move_sample(db: &Connection, old: &Path, new: &Path) -> Result<(), Box<dyn Error>> {
    if new.exists() {
        return Err(format!("{} already exists", new.display()).into());
    }
    match std::fs::rename(old, new) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(old, new)?;
            std::fs::remove_file(old)?;
        }
        Err(error) => return Err(error.into()),
    }
    db::rename(db, old, new)?;
    Ok(())
}

/// Sends a sample's file to the system's trash and forgets it in the
/// database (tags, collections, waveform): put back from the trash, it comes
/// back as a new sample, without them.
pub fn trash_sample(db: &Connection, path: &Path) -> Result<(), Box<dyn Error>> {
    trash::delete(path)?;
    db::forget(db, path)?;
    Ok(())
}

/// Converts a sample with ffmpeg (see `convert`), next to the original, and
/// adds the copy to the library with the original's tags and collections.
/// Returns its path.
pub fn convert_sample(
    db: &Connection,
    path: &Path,
    options: &convert::Options,
) -> Result<PathBuf, Box<dyn Error>> {
    let original = read_info(path)?;
    // A sample already mono is only converted: in a mixed selection, "mono"
    // concerns the stereo ones.
    let mut options = options.clone();
    if original.channels == Some(1) {
        options.mono = false;
    }
    let converted = convert::convert(path, &options, original.bits)?;
    // Saved at once rather than left to the folder watcher, so that the copy
    // gets the original's tags and collections now.
    let file_id = db::save(db, &read_info(&converted)?)?;
    if let Some(original_id) = db::file_id(db, path)? {
        db::add_tags(db, file_id, &db::tags_of(db, original_id)?)?;
        for collection in db::collections_holding(db, &[original_id])? {
            db::add_to_collection(db, collection, &[file_id])?;
        }
    }
    Ok(converted)
}

/// True for `._kick.wav`: metadata that macOS leaves next to `kick.wav` on a
/// disk that cannot store it (USB stick, network share). Not sound.
pub fn is_mac_metadata(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.starts_with("._"))
}

/// True for a hidden file or folder (name starting with a dot).
fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// True when `path` is hidden, or inside a hidden folder, below one of the
/// scanned `roots`. Such files are never samples of the library: the trash of
/// a disk scanned at its root (`.Trash-1000` on Linux, `.Trashes` on Mac,
/// where a sample moved to the trash would otherwise come back), the system
/// folders of external disks (`.Spotlight-V100`, `.fseventsd`), a conversion
/// being written (`.kick.wav.part`). A scanned folder may itself be hidden:
/// only what lies below it counts.
pub fn is_hidden_below(path: &Path, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .filter_map(|root| path.strip_prefix(root).ok())
        .any(|below| below.components().any(|part| is_hidden(part.as_os_str())))
}

/// The entries of a walk of `folder` that `index` looks at: hidden files and
/// folders below it are skipped (see `is_hidden_below`).
fn walk(folder: &Path) -> impl Iterator<Item = walkdir::DirEntry> {
    WalkDir::new(folder)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || !is_hidden(entry.file_name()))
        // An unreadable folder (missing permissions…) must not stop everything.
        .filter_map(|entry| entry.ok())
}

/// True if the file has a known audio extension (case-insensitive).
pub fn is_audio(path: &Path) -> bool {
    if is_mac_metadata(path) {
        return false;
    }

    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// The result of `scan`.
pub struct Scan {
    /// The scanned folder, as an absolute path.
    pub folder: PathBuf,
    pub samples: Vec<SampleInfo>,
    pub unreadable: usize,
}

/// Saves the folder (followed by `samplr watch`) and the samples it contains
/// to the database. `progress` is called after each audio file with the
/// number gone through so far and the total, counted first.
pub fn scan(
    db: &mut Connection,
    folder: &Path,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Scan, Box<dyn Error>> {
    if !folder.is_dir() {
        return Err(format!("\"{}\" is not a folder", folder.display()).into());
    }
    // Absolute path: the database must find the files again whatever folder
    // samplr is run from.
    let folder = dunce::canonicalize(folder)?;

    let total = count_audio_files(&folder);
    db::add_folder(db, &folder)?;
    let (samples, unreadable) = index(db, &folder, &mut |done| progress(done, total));
    Ok(Scan {
        folder,
        samples,
        unreadable,
    })
}

/// A scanned folder, as `samplr folders` and the interface describe it.
#[derive(Serialize)]
pub struct Folder {
    pub path: PathBuf,
    /// Samples shown from this folder.
    pub samples: u32,
    /// `false` when the folder is not on disk (unplugged disk, deleted).
    pub found: bool,
}

/// The scanned folders, in alphabetical order.
pub fn folders(db: &Connection) -> Result<Vec<Folder>, Box<dyn Error>> {
    let mut list = Vec::new();
    for path in db::folders(db)? {
        list.push(Folder {
            samples: db::count_under(db, &path)?,
            found: path.is_dir(),
            path,
        });
    }
    Ok(list)
}

/// Removes a scanned folder from the library (see `db::remove_folder`):
/// no longer followed, its samples hidden but kept with their tags. Returns
/// the number of samples hidden. The audio files are not touched.
pub fn remove_folder(db: &mut Connection, folder: &Path) -> Result<usize, Box<dyn Error>> {
    // Scanned folders are saved as absolute paths. An unplugged disk can no
    // longer be resolved: the path is then taken as typed, made absolute.
    let folder = match dunce::canonicalize(folder) {
        Ok(folder) => folder,
        Err(_) => std::path::absolute(folder)?,
    };
    let transaction = db.transaction()?;
    let hidden = db::remove_folder(&transaction, &folder)?;
    transaction.commit()?;
    Ok(hidden)
}

/// The number of audio files in `folder` and its subfolders, without reading
/// them: fast, used to show a scan's progress.
pub fn count_audio_files(folder: &Path) -> usize {
    walk(folder)
        .filter(|entry| entry.file_type().is_file() && is_audio(entry.path()))
        .count()
}

/// Walks `folder` and its subfolders and saves each sample to the database.
/// Returns the saved samples and the number of unreadable files. `progress`
/// is called after each audio file, readable or not, with the number of audio
/// files gone through so far.
pub fn index(
    db: &Connection,
    folder: &Path,
    progress: &mut dyn FnMut(usize),
) -> (Vec<SampleInfo>, usize) {
    let mut samples = Vec::new();
    let mut unreadable = 0;
    let mut done = 0;
    // Files are read outside any transaction, and saved by batches, each in
    // a short transaction of its own: the write lock is held a few
    // milliseconds at a time, not for the whole walk (minutes on a slow disk
    // or a network share), during which the interface and the other commands
    // could not write, then failed. A batch still writes to disk once rather
    // than once per sample.
    let mut batch = Vec::with_capacity(SAVE_BATCH);

    for entry in walk(folder) {
        let path = entry.path();

        if !entry.file_type().is_file() || !is_audio(path) {
            continue;
        }

        match read_info(path) {
            Ok(info) => batch.push(info),
            Err(_) => unreadable += 1,
        }
        done += 1;
        progress(done);
        if batch.len() == SAVE_BATCH {
            save_batch(db, &mut batch, &mut samples, &mut unreadable);
        }
    }
    save_batch(db, &mut batch, &mut samples, &mut unreadable);
    (samples, unreadable)
}

/// Samples saved per transaction by `index`.
const SAVE_BATCH: usize = 200;

/// Saves `batch` in one transaction, moving each sample saved into `samples`
/// and counting the others as unreadable. Must not be called inside a
/// transaction.
fn save_batch(
    db: &Connection,
    batch: &mut Vec<SampleInfo>,
    samples: &mut Vec<SampleInfo>,
    unreadable: &mut usize,
) {
    if batch.is_empty() {
        return;
    }
    let saved = (|| -> Result<Vec<SampleInfo>, Box<dyn Error>> {
        let transaction = db.unchecked_transaction()?;
        let mut saved = Vec::with_capacity(batch.len());
        for info in batch.drain(..) {
            match db::save(&transaction, &info) {
                Ok(_) => saved.push(info),
                Err(error) => {
                    eprintln!("{}: {error}", info.path.display());
                    *unreadable += 1;
                }
            }
        }
        transaction.commit()?;
        Ok(saved)
    })();
    match saved {
        Ok(saved) => samples.extend(saved),
        Err(error) => {
            eprintln!("saving {} sample(s): {error}", batch.len());
            *unreadable += batch.len();
            batch.clear();
        }
    }
}

/// Whether the file is a WAV, after its extension.
pub(crate) fn is_wav(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

/// Opens an audio file with symphonia, reading only its headers.
pub(crate) fn probe(path: &Path) -> Result<ProbeResult, Box<dyn Error>> {
    let file = File::open(path)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    // The extension is given as a hint to help symphonia recognise the format.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    Ok(symphonia::default::get_probe().format(
        &hint,
        stream,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?)
}

/// Opens an audio file with symphonia and reads its characteristics without
/// decoding the whole sound (it is fast).
pub fn read_info(path: &Path) -> Result<SampleInfo, Box<dyn Error>> {
    let probed = probe(path)?;

    let track = probed
        .format
        .default_track()
        .ok_or("no audio track in the file")?;
    let params = &track.codec_params;

    // Duration = number of frames / sample rate.
    let duration_secs = match (params.n_frames, params.sample_rate) {
        (Some(frames), Some(rate)) => Some(frames as f64 / rate as f64),
        _ => None,
    };
    // symphonia's short name for it: "pcm_s24le", "flac", "mp3", "vorbis"…
    let codec = symphonia::default::get_codecs()
        .get_codec(params.codec)
        .map(|descriptor| descriptor.short_name.to_string());

    Ok(SampleInfo {
        path: path.to_path_buf(),
        duration_secs,
        sample_rate: params.sample_rate,
        channels: params.channels.map(|c| c.count() as u32),
        bits: params.bits_per_sample,
        tags: Vec::new(),
        favorite: false,
        collections: Vec::new(),
        bpm: None,
        bpm_source: None,
        bpm_fixed: false,
        key: None,
        looped: false,
        loop_source: None,
        sound_type: None,
        type_source: None,
        lufs: None,
        peak_db: None,
        codec,
        acid: acid::read(path),
    })
}

/// What `update_waveform` did.
pub enum WaveformUpdate {
    Computed,
    UpToDate,
    /// Deleted file, or disk unplugged.
    Missing,
    /// The file could not be decoded: the reason.
    Unreadable(String),
}

/// Computes and caches the waveform of a sample of the database if it is
/// missing or stale.
pub fn update_waveform(
    db: &Connection,
    file_id: i64,
    path: &Path,
) -> Result<WaveformUpdate, Box<dyn Error>> {
    // The stamp is read before decoding: if the file changes meanwhile, the
    // saved stamp will no longer match and the waveform will be computed
    // again next time.
    let Ok(stamp) = db::FileStamp::read(path) else {
        return Ok(WaveformUpdate::Missing);
    };
    if db::has_waveform(db, file_id, stamp)? {
        return Ok(WaveformUpdate::UpToDate);
    }
    match waveform::compute(path) {
        Ok((peaks, measures)) => {
            db::save_waveform(db, file_id, stamp, &peaks, measures)?;
            Ok(WaveformUpdate::Computed)
        }
        Err(error) => Ok(WaveformUpdate::Unreadable(error.to_string())),
    }
}

/// The folders where Splice keeps the sounds it downloads, those present
/// on this computer. Splice documents its folder as `~/Splice` on the Mac
/// and in the user's folder on Windows, the sounds inside; its plugin keeps
/// its own elsewhere (`~/Library/Splice/Plug-in/samples` on the Mac,
/// `AppData/Roaming/Splice/Plug-in/samples` on Windows). There is no Splice
/// for Linux: none is found there, unless copied by hand.
pub fn splice_folders() -> Vec<PathBuf> {
    let home = dirs::home_dir();
    let mut candidates = Vec::new();
    if let Some(home) = &home {
        candidates.push(home.join("Splice").join("sounds"));
        candidates.push(home.join("Splice"));
        candidates.push(home.join("Library/Splice/Plug-in/samples"));
    }
    if let Some(documents) = dirs::document_dir() {
        candidates.push(documents.join("Splice").join("sounds"));
        candidates.push(documents.join("Splice"));
    }
    if let Some(config) = dirs::config_dir() {
        candidates.push(config.join("Splice/Plug-in/samples"));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        // `~/Splice/sounds` rather than `~/Splice` when both are there: the
        // latter also holds presets and projects.
        if candidate.is_dir() && !found.iter().any(|kept| kept.starts_with(&candidate)) {
            found.push(dunce::canonicalize(&candidate).unwrap_or(candidate));
        }
    }
    found
}

/// The tempos a sample's name, ACID chunk and length give (see
/// `analysis`), whatever is used for it: what "Detect BPM" offers.
#[derive(Serialize)]
pub struct TempoMethods {
    pub name: Option<u32>,
    pub file: Option<u32>,
    pub length: Option<u32>,
}

/// Reads the file's headers only, never the sound.
pub fn tempo_methods(path: &Path) -> Result<TempoMethods, Box<dyn Error>> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let info = read_info(path)?;
    let seconds = info.duration_secs;
    Ok(TempoMethods {
        name: analysis::tempo_from_name(stem),
        file: info
            .acid
            .as_ref()
            .zip(seconds)
            .and_then(|(acid, seconds)| acid::tempo(acid.beats, seconds)),
        length: seconds.and_then(analysis::tempo_from_length),
    })
}

/// A sample's tempo by the method asked ("name", "file", "length" or
/// "sound"), whatever the others say. "sound" decodes the file again (see
/// `pulse`) and caches its waveform on the way. `None` when the method gives
/// none: no tempo in the name, no ACID chunk, a length of no whole tempo, a
/// sound too short or longer than a loop can be.
pub fn detect_tempo(
    db: &Connection,
    file_id: i64,
    path: &Path,
    method: &str,
) -> Result<Option<u32>, Box<dyn Error>> {
    let methods = || tempo_methods(path);
    match analysis::Source::from_name(method) {
        Some(analysis::Source::Name) => return Ok(methods()?.name),
        Some(analysis::Source::File) => return Ok(methods()?.file),
        Some(analysis::Source::Length) => return Ok(methods()?.length),
        Some(analysis::Source::Sound) => {}
        _ => return Err(format!("unknown method “{method}”").into()),
    }
    let stamp = db::FileStamp::read(path)?;
    let (peaks, measures) = waveform::compute(path)?;
    db::save_waveform(db, file_id, stamp, &peaks, measures)?;
    Ok(measures.tempo)
}

/// The waveform of a sample of the database: the cached one if the file has
/// not changed since, otherwise computed now and cached.
pub fn peaks(
    db: &Connection,
    file_id: i64,
    path: &Path,
) -> Result<Vec<waveform::Peak>, Box<dyn Error>> {
    let stamp = db::FileStamp::read(path)?;
    if let Some(peaks) = db::waveform(db, file_id, stamp)? {
        return Ok(peaks);
    }
    let (peaks, measures) = waveform::compute(path)?;
    db::save_waveform(db, file_id, stamp, &peaks, measures)?;
    Ok(peaks)
}
