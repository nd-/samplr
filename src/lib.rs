// samplr's shared code: what the command line and the graphical interface
// both need (database, waveforms, reading audio files).

pub mod db;
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

/// Walks `folder` and its subfolders and saves each sample to the database.
/// Returns the saved samples and the number of unreadable files.
pub fn index(db: &Connection, folder: &Path) -> (Vec<SampleInfo>, usize) {
    let mut samples = Vec::new();
    let mut unreadable = 0;

    for entry in WalkDir::new(folder).sort_by_file_name() {
        // An unreadable folder (missing permissions…) must not stop everything.
        let Ok(entry) = entry else { continue };
        let path = entry.path();

        if !entry.file_type().is_file() || !is_audio(path) {
            continue;
        }

        let Ok(info) = read_info(path) else {
            unreadable += 1;
            continue;
        };
        if let Err(error) = db::save(db, &info) {
            eprintln!("{}: {error}", path.display());
            unreadable += 1;
            continue;
        }
        samples.push(info);
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
    })
}
