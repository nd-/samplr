// samplr's shared code: what the command line and the graphical interface
// both need (database, waveforms, reading audio files).

pub mod db;
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
use symphonia::core::probe::Hint;
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
}

/// True for `._kick.wav`: metadata that macOS leaves next to `kick.wav` on a
/// disk that cannot store it (USB stick, network share). Not sound.
pub fn is_mac_metadata(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.starts_with("._"))
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

    // A transaction groups all the writes: SQLite writes to disk only once,
    // on `commit`, instead of once per sample (hundreds of times slower on a
    // big folder).
    let total = count_audio_files(&folder);
    let transaction = db.transaction()?;
    db::add_folder(&transaction, &folder)?;
    let (samples, unreadable) = index(&transaction, &folder, &mut |done| progress(done, total));
    transaction.commit()?;
    Ok(Scan {
        folder,
        samples,
        unreadable,
    })
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
    WalkDir::new(folder)
        .into_iter()
        .filter_map(|entry| entry.ok())
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

    for entry in WalkDir::new(folder).sort_by_file_name() {
        // An unreadable folder (missing permissions…) must not stop everything.
        let Ok(entry) = entry else { continue };
        let path = entry.path();

        if !entry.file_type().is_file() || !is_audio(path) {
            continue;
        }

        match read_info(path) {
            Ok(info) => match db::save(db, &info) {
                Ok(_) => samples.push(info),
                Err(error) => {
                    eprintln!("{}: {error}", path.display());
                    unreadable += 1;
                }
            },
            Err(_) => unreadable += 1,
        }
        done += 1;
        progress(done);
    }
    (samples, unreadable)
}

/// Opens an audio file with symphonia and reads its characteristics without
/// decoding the whole sound (it is fast).
pub fn read_info(path: &Path) -> Result<SampleInfo, Box<dyn Error>> {
    let file = File::open(path)?; // `?` hands the error back to the caller, if any
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    // The extension is given as a hint to help symphonia recognise the format.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        stream,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

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

    Ok(SampleInfo {
        path: path.to_path_buf(),
        duration_secs,
        sample_rate: params.sample_rate,
        channels: params.channels.map(|c| c.count() as u32),
        bits: params.bits_per_sample,
        tags: Vec::new(),
        favorite: false,
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
    if db::waveform(db, file_id, stamp)?.is_some() {
        return Ok(WaveformUpdate::UpToDate);
    }
    match waveform::compute(path) {
        Ok(peaks) => {
            db::save_waveform(db, file_id, stamp, &peaks)?;
            Ok(WaveformUpdate::Computed)
        }
        Err(error) => Ok(WaveformUpdate::Unreadable(error.to_string())),
    }
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
    let peaks = waveform::compute(path)?;
    db::save_waveform(db, file_id, stamp, &peaks)?;
    Ok(peaks)
}
