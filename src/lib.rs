// samplr's shared code: what the command line and the graphical interface
// both need (database, waveforms, reading audio files).

pub mod acid;
pub mod analysis;
pub mod convert;
pub mod db;
pub mod info;
pub mod pulse;
pub mod query;
pub mod sound_type;
pub mod timbre;
pub mod watch;
pub mod waveform;

#[cfg(test)]
mod test_support;

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
        // Without it, an MP3 or an OGG is a few tens of milliseconds too long
        // (the encoder's delay and padding): enough to miss a loop's tempo
        // from its length, which allows 2 %.
        &FormatOptions {
            enable_gapless: true,
            ..Default::default()
        },
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

/// The samples shown, ranked by how close their timbre is to the sample
/// `file_id`'s, nearest first, with their distance (see `timbre::rank`); the
/// sample itself left out. Its timbre is measured first if needed; samples
/// not measured yet, or silent, are left out.
pub fn similar(
    db: &Connection,
    file_id: i64,
    path: &Path,
) -> Result<Vec<(PathBuf, f32)>, Box<dyn Error>> {
    match update_waveform(db, file_id, path)? {
        WaveformUpdate::Missing => return Err(format!("{} not found", path.display()).into()),
        WaveformUpdate::Unreadable(error) => return Err(error.into()),
        WaveformUpdate::Computed | WaveformUpdate::UpToDate => {}
    }
    let target = db::timbre(db, file_id)?
        .ok_or_else(|| format!("{}: silent, no timbre to compare", path.display()))?;
    let timbres = db::timbres(db)?;
    let library: Vec<(usize, timbre::Fingerprint)> = timbres
        .iter()
        .enumerate()
        .map(|(index, (_, _, fingerprint))| (index, *fingerprint))
        .collect();
    Ok(timbre::rank(&target, &library)
        .into_iter()
        .filter(|(index, _)| timbres[*index].0 != file_id)
        .map(|(index, distance)| (timbres[index].1.clone(), distance))
        .collect())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, encode, memory_db, write_sound, write_wav};
    use std::fs;

    #[test]
    fn moving_a_sample_keeps_its_id_and_tags() {
        let dir = TempDir::new();
        let (old, new) = (
            dir.path().join("kick.wav"),
            dir.path().join("drums/kick.wav"),
        );
        write_wav(&old);
        fs::create_dir(new.parent().unwrap()).unwrap();
        let db = memory_db();
        let id = db::save(&db, &read_info(&old).unwrap()).unwrap();
        db::add_tags(&db, id, &["drums".to_string()]).unwrap();

        move_sample(&db, &old, &new).unwrap();
        assert!(!old.exists() && new.is_file());
        assert_eq!(db::file_id(&db, &old).unwrap(), None);
        assert_eq!(db::file_id(&db, &new).unwrap(), Some(id));
        assert_eq!(db::tags_of(&db, id).unwrap(), ["drums"]);
    }

    #[test]
    fn moving_a_sample_never_overwrites_a_file() {
        let dir = TempDir::new();
        let (old, new) = (dir.path().join("a.wav"), dir.path().join("b.wav"));
        write_wav(&old);
        fs::write(&new, b"not to be lost").unwrap();
        let db = memory_db();
        let id = db::save(&db, &read_info(&old).unwrap()).unwrap();

        assert!(move_sample(&db, &old, &new).is_err());
        assert!(old.is_file());
        assert_eq!(fs::read(&new).unwrap(), b"not to be lost");
        assert_eq!(db::file_id(&db, &old).unwrap(), Some(id));
    }

    #[test]
    fn a_failed_move_leaves_the_database_alone() {
        let dir = TempDir::new();
        let (old, new) = (dir.path().join("a.wav"), dir.path().join("missing/a.wav"));
        write_wav(&old);
        let db = memory_db();
        let id = db::save(&db, &read_info(&old).unwrap()).unwrap();

        // The destination folder does not exist: the rename fails.
        assert!(move_sample(&db, &old, &new).is_err());
        assert!(old.is_file());
        assert_eq!(db::file_id(&db, &old).unwrap(), Some(id));
    }

    #[test]
    fn which_files_are_samples() {
        assert!(is_audio(Path::new("/lib/kick.WAV")));
        assert!(is_audio(Path::new("/lib/pad.aiff")));
        assert!(!is_audio(Path::new("/lib/notes.txt")));
        assert!(!is_audio(Path::new("/lib/kick")));
        // Left by macOS next to `kick.wav`.
        assert!(!is_audio(Path::new("/lib/._kick.wav")));

        let roots = [PathBuf::from("/lib"), PathBuf::from("/disk/.samples")];
        assert!(is_hidden_below(
            Path::new("/lib/.Trash-1000/kick.wav"),
            &roots
        ));
        assert!(is_hidden_below(
            Path::new("/lib/drums/.kick.wav.part"),
            &roots
        ));
        assert!(!is_hidden_below(Path::new("/lib/drums/kick.wav"), &roots));
        // A scanned folder may itself be hidden: only what lies below counts.
        assert!(!is_hidden_below(
            Path::new("/disk/.samples/kick.wav"),
            &roots
        ));
        // Outside every scanned folder: not this function's concern.
        assert!(!is_hidden_below(
            Path::new("/elsewhere/.x/kick.wav"),
            &roots
        ));
    }

    #[test]
    fn scanning_counts_first_then_reports_each_file() {
        let dir = TempDir::new();
        write_wav(&dir.path().join("a.wav"));
        write_wav(&dir.path().join("drums/b.wav"));
        fs::write(dir.path().join("broken.wav"), "not a sound").unwrap();
        fs::write(dir.path().join("notes.txt"), "not audio").unwrap();
        let mut db = memory_db();

        let mut calls = Vec::new();
        let scan = scan(&mut db, dir.path(), &mut |done, total| {
            calls.push((done, total))
        })
        .unwrap();
        // Every audio file, readable or not, the total known from the start.
        assert_eq!(calls, [(1, 3), (2, 3), (3, 3)]);
        assert_eq!(scan.folder, dir.path());
        assert_eq!((scan.samples.len(), scan.unreadable), (2, 1));
        assert_eq!(db::folders(&db).unwrap(), [dir.path().to_path_buf()]);
    }

    #[test]
    fn scanning_a_file_is_refused() {
        let dir = TempDir::new();
        let file = dir.path().join("a.wav");
        write_wav(&file);
        assert!(scan(&mut memory_db(), &file, &mut |_, _| {}).is_err());
    }

    #[test]
    fn indexing_saves_beyond_one_batch() {
        let dir = TempDir::new();
        // More than two batches: the last one, incomplete, is saved too.
        let count = 2 * SAVE_BATCH + 50;
        for i in 0..count {
            write_sound(&dir.path().join(format!("{i:04}.wav")), 1, &[0; 10]);
        }
        let db = memory_db();
        let (samples, unreadable) = index(&db, dir.path(), &mut |_| {});
        assert_eq!((samples.len(), unreadable), (count, 0));
        assert_eq!(db::all_files(&db).unwrap().len(), count);
    }

    #[test]
    fn a_folder_on_an_unplugged_disk_can_still_be_removed() {
        let dir = TempDir::new();
        let disk = dir.path().join("disk");
        write_wav(&disk.join("kick.wav"));
        let mut db = memory_db();
        scan(&mut db, &disk, &mut |_, _| {}).unwrap();
        fs::remove_dir_all(&disk).unwrap();

        // The path no longer resolves: taken as given.
        assert_eq!(remove_folder(&mut db, &disk).unwrap(), 1);
        assert!(db::folders(&db).unwrap().is_empty());
    }

    #[test]
    fn a_converted_copy_gets_the_tags_and_collections() {
        if convert::ffmpeg().is_none() {
            eprintln!("ffmpeg not found: conversion not tested");
            return;
        }
        let dir = TempDir::new();
        // Already mono: "mono" is ignored, and not added to the name.
        let source = dir.path().join("kick.wav");
        write_wav(&source);
        let db = memory_db();
        let id = db::save(&db, &read_info(&source).unwrap()).unwrap();
        db::add_tags(&db, id, &["drums".to_string()]).unwrap();
        let collection = db::create_collection(&db, "Set").unwrap();
        db::add_to_collection(&db, collection, &[id]).unwrap();

        let options = convert::Options {
            format: "flac".to_string(),
            bits: None,
            mp3_bitrate: 320,
            ogg_quality: 8,
            mono: true,
        };
        let copy = convert_sample(&db, &source, &options).unwrap();
        assert_eq!(copy, dir.path().join("kick.flac"));
        let copy_id = db::file_id(&db, &copy).unwrap().unwrap();
        assert_eq!(db::tags_of(&db, copy_id).unwrap(), ["drums"]);
        assert_eq!(
            db::collections_holding(&db, &[copy_id]).unwrap(),
            [collection]
        );
    }

    /// 2.4 s of silence named for 90 BPM: its length says 100 (4 beats).
    fn loop_90(dir: &TempDir) -> PathBuf {
        let path = dir.path().join("Loop 90BPM.wav");
        write_sound(&path, 1, &vec![0; 105_840]);
        path
    }

    #[test]
    fn each_tempo_method_on_its_own() {
        let dir = TempDir::new();
        let path = loop_90(&dir);
        let methods = tempo_methods(&path).unwrap();
        assert_eq!(
            (methods.name, methods.file, methods.length),
            (Some(90), None, Some(100))
        );

        let db = memory_db();
        let id = db::save(&db, &read_info(&path).unwrap()).unwrap();
        assert_eq!(detect_tempo(&db, id, &path, "name").unwrap(), Some(90));
        assert_eq!(detect_tempo(&db, id, &path, "length").unwrap(), Some(100));
        assert_eq!(detect_tempo(&db, id, &path, "file").unwrap(), None);
        assert!(detect_tempo(&db, id, &path, "guess").is_err());
    }

    #[test]
    fn detecting_from_the_sound_caches_the_waveform() {
        let dir = TempDir::new();
        let path = loop_90(&dir);
        let db = memory_db();
        let id = db::save(&db, &read_info(&path).unwrap()).unwrap();
        let stamp = db::FileStamp::read(&path).unwrap();
        assert!(!db::has_waveform(&db, id, stamp).unwrap());
        // Silence has no pulse.
        assert_eq!(detect_tempo(&db, id, &path, "sound").unwrap(), None);
        assert!(db::has_waveform(&db, id, stamp).unwrap());
    }

    #[test]
    fn the_timbre_is_stored_with_the_waveform() {
        let dir = TempDir::new();
        let db = memory_db();
        let tone = dir.path().join("tone.wav");
        let samples: Vec<i16> = (0..44_100)
            .map(|i| (8000.0 * (i as f32 * 0.0627).sin()) as i16)
            .collect();
        write_sound(&tone, 1, &samples);
        let silent = dir.path().join("silent.wav");
        write_wav(&silent);
        let tone_id = db::save(&db, &read_info(&tone).unwrap()).unwrap();
        let silent_id = db::save(&db, &read_info(&silent).unwrap()).unwrap();
        assert_eq!(db::timbre(&db, tone_id).unwrap(), None);
        update_waveform(&db, tone_id, &tone).unwrap();
        update_waveform(&db, silent_id, &silent).unwrap();
        let timbre = db::timbre(&db, tone_id).unwrap().unwrap();
        assert!(timbre.0[timbre::FLATNESS] < 0.1);
        assert_eq!(db::timbre(&db, silent_id).unwrap(), None);
    }

    #[test]
    fn peaks_come_from_the_cache_until_the_file_changes() {
        let dir = TempDir::new();
        let path = dir.path().join("kick.wav");
        write_wav(&path);
        let db = memory_db();
        let id = db::save(&db, &read_info(&path).unwrap()).unwrap();
        assert_eq!(peaks(&db, id, &path).unwrap().len(), 1024);

        // A cached waveform that could not come from the file: read back as
        // it is, so the file was not decoded again.
        let stamp = db::FileStamp::read(&path).unwrap();
        let marked = vec![(-1.0, 1.0); 3];
        db::save_waveform(&db, id, stamp, &marked, waveform::Measures::default()).unwrap();
        assert_eq!(peaks(&db, id, &path).unwrap(), marked);

        // Another size: the cache is stale, the file decoded again.
        write_sound(&path, 1, &[0; 20]);
        assert_eq!(peaks(&db, id, &path).unwrap(), vec![(0.0, 0.0); 20]);
    }

    /// One second of stereo at 44.1 kHz, then encoded with ffmpeg as
    /// `name`; `None` when ffmpeg is not installed.
    fn encoded(dir: &TempDir, name: &str, args: &[&str]) -> Option<SampleInfo> {
        let source = dir.path().join("source.wav");
        let samples: Vec<i16> = (0..88_200).map(|i| ((i % 200) * 100) as i16).collect();
        write_sound(&source, 2, &samples);
        let target = dir.path().join(name);
        encode(&source, &target, args).then(|| read_info(&target).unwrap())
    }

    #[test]
    fn every_format_read_without_decoding() {
        let dir = TempDir::new();
        let formats: [(&str, &[&str], &str, Option<u32>); 4] = [
            ("a.aiff", &["-c:a", "pcm_s24be"], "pcm_s24be", Some(24)),
            (
                "a.flac",
                &[
                    "-c:a",
                    "flac",
                    "-sample_fmt",
                    "s32",
                    "-bits_per_raw_sample",
                    "24",
                ],
                "flac",
                Some(24),
            ),
            // Lossy: no bit depth.
            (
                "a.mp3",
                &["-c:a", "libmp3lame", "-b:a", "192k"],
                "mp3",
                None,
            ),
            ("a.ogg", &["-c:a", "libvorbis", "-q:a", "5"], "vorbis", None),
        ];
        for (name, args, codec, bits) in formats {
            let Some(info) = encoded(&dir, name, args) else {
                return;
            };
            assert_eq!(info.codec.as_deref(), Some(codec), "{name}");
            assert_eq!(info.bits, bits, "{name}");
            assert_eq!(
                (info.channels, info.sample_rate),
                (Some(2), Some(44_100)),
                "{name}"
            );
            let seconds = info.duration_secs.unwrap();
            // Lossy too, without the encoder's delay and padding; symphonia
            // counts one frame too many in an AIFF.
            assert!((1.0..=1.001).contains(&seconds), "{name}: {seconds} s");
        }
    }
}
