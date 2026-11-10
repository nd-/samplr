// samplr's SQLite database: the known samples and their tags.
// Tags live here, never inside the audio files.

use std::env;
use std::error::Error;
use std::fs;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OptionalExtension, named_params, params, params_from_iter};
use serde::Serialize;

use crate::waveform::{self, Peak};
use crate::{SampleInfo, acid, analysis, sound_type};

/// Location of the database file: the system's data folder
/// (`~/.local/share/samplr` on Linux, `~/Library/Application Support/samplr`
/// on Mac). The `SAMPLR_DB` environment variable points to another file, to
/// experiment without touching the real one.
fn db_path() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = env::var_os("SAMPLR_DB") {
        return Ok(PathBuf::from(path));
    }
    let folder = dirs::data_dir()
        .ok_or("system data folder not found")?
        .join("samplr");
    fs::create_dir_all(&folder)?;
    Ok(folder.join("samplr.db"))
}

/// Opens the database (creating it on first launch) and makes sure the tables exist.
pub fn open() -> Result<Connection, Box<dyn Error>> {
    let db = Connection::open(db_path()?)?;
    set_up(&db)?;
    Ok(db)
}

/// Brings an open database to the current schema and registers the
/// functions the queries use. Separate from `open` so that the tests run it
/// on a database in memory.
pub(crate) fn set_up(db: &Connection) -> Result<(), Box<dyn Error>> {
    // SQLite only checks foreign keys when asked to, and the setting has to
    // be repeated every time the database is opened.
    db.execute_batch(
        "PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS files (
            id            INTEGER PRIMARY KEY,
            path          TEXT NOT NULL UNIQUE,
            duration_secs REAL,
            sample_rate   INTEGER,
            channels      INTEGER,
            bits          INTEGER
        );

        CREATE TABLE IF NOT EXISTS tags (
            id   INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS file_tags (
            file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (file_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS folders (
            id   INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS waveforms (
            file_id  INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
            size     INTEGER NOT NULL,
            modified INTEGER NOT NULL,
            peaks    BLOB NOT NULL
        );",
    )?;
    migrate(db)?;

    // SQLite's `lower` only knows ASCII: "Été" stays "Été", and a search for
    // "été" missed it. The search compares through this one, Rust's
    // lowercase, which knows every alphabet (tags are stored with it too).
    db.create_scalar_function(
        "unicode_lower",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| Ok(context.get::<String>(0)?.to_lowercase()),
    )?;
    Ok(())
}

/// The schema version `migrate` brings a database to: the last step's.
const SCHEMA_VERSION: i64 = 12;

/// What a waveform's row holds, in `waveforms.measured`: 1 the loudness, 2
/// the tempo detected too. A row of an older version is stale: computed
/// again, once, in the background, its loudness shown meanwhile.
const MEASURED: i64 = 2;

/// Brings a database created by an older samplr up to date. Its version is
/// kept by SQLite (`user_version`, 0 for a new database); each step runs once.
/// A new database goes through every step too: the tables above are those of
/// version 0.
fn migrate(db: &Connection) -> Result<(), Box<dyn Error>> {
    // Up to date, the usual case: no lock taken. Taking the write lock at
    // every opening made every command, even a search, wait behind any other
    // process writing (a long scan), then fail after 5 s.
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    // `IMMEDIATE` takes the write lock before reading the version again: the
    // app and a `samplr` command opening the database together cannot both
    // run the same step.
    db.execute_batch("BEGIN IMMEDIATE")?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 1 {
        // Samples of a folder removed from the library: hidden, but kept with
        // their tags in case the folder is added again.
        db.execute_batch(
            "ALTER TABLE files ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
             PRAGMA user_version = 1;",
        )?;
    }
    if version < 2 {
        db.execute_batch(
            "ALTER TABLE files ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0;
             PRAGMA user_version = 2;",
        )?;
    }
    if version < 3 {
        db.execute_batch(
            "CREATE TABLE collections (
                 id   INTEGER PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE
             );
             CREATE TABLE collection_files (
                 collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
                 file_id       INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                 PRIMARY KEY (collection_id, file_id)
             );
             PRAGMA user_version = 3;",
        )?;
    }
    if version < 4 {
        // A palette to colour tags and collections, filled with a first set
        // of colours that the settings panel lets one change. Deleting a
        // colour leaves its tags and collections without colour.
        db.execute_batch(
            "CREATE TABLE colors (
                 id       INTEGER PRIMARY KEY,
                 name     TEXT NOT NULL,
                 hex      TEXT NOT NULL,
                 position INTEGER NOT NULL
             );
             INSERT INTO colors (name, hex, position) VALUES
                 ('Red', '#e5484d', 1), ('Orange', '#f76b15', 2), ('Amber', '#ffc53d', 3),
                 ('Green', '#30a46c', 4), ('Teal', '#12a594', 5), ('Blue', '#0090ff', 6),
                 ('Indigo', '#3e63dd', 7), ('Purple', '#8e4ec6', 8), ('Pink', '#d6409f', 9),
                 ('Grey', '#8b8d98', 10);
             ALTER TABLE tags ADD COLUMN color_id INTEGER REFERENCES colors(id) ON DELETE SET NULL;
             ALTER TABLE collections ADD COLUMN color_id INTEGER REFERENCES colors(id) ON DELETE SET NULL;
             PRAGMA user_version = 4;",
        )?;
    }
    if version < 5 {
        // Tempo and key set by hand. NULL: worked out from the name and the
        // length (see `analysis`); 0 and '' mean "none", to silence a wrong
        // guess.
        db.execute_batch(
            "ALTER TABLE files ADD COLUMN bpm INTEGER;
             ALTER TABLE files ADD COLUMN key TEXT;
             PRAGMA user_version = 5;",
        )?;
    }
    if version < 6 {
        // The file's ACID chunk (see `acid`), read when the file is indexed:
        // its length in beats, its root note, loop (1) or one-shot (0). NULL
        // when the file has none. Filled for existing samples by the next
        // catch-up, which reads every file again.
        db.execute_batch(
            "ALTER TABLE files ADD COLUMN acid_beats INTEGER;
             ALTER TABLE files ADD COLUMN acid_root TEXT;
             ALTER TABLE files ADD COLUMN acid_loop INTEGER;
             PRAGMA user_version = 6;",
        )?;
    }
    if version < 7 {
        // Loop (1) or one-shot (0) set by hand; NULL: worked out (see
        // `analysis::looped`).
        db.execute_batch(
            "ALTER TABLE files ADD COLUMN looped INTEGER;
             PRAGMA user_version = 7;",
        )?;
    }
    if version < 8 {
        // Loudness, measured with the waveform (see `waveform::compute`):
        // integrated LUFS and peak in dBFS, NULL when they cannot be
        // measured. `measured` is 0 for a waveform saved before: it counts
        // as stale and is computed again, once, in the background.
        db.execute_batch(
            "ALTER TABLE waveforms ADD COLUMN lufs REAL;
             ALTER TABLE waveforms ADD COLUMN peak_db REAL;
             ALTER TABLE waveforms ADD COLUMN measured INTEGER NOT NULL DEFAULT 0;
             PRAGMA user_version = 8;",
        )?;
    }
    if version < 9 {
        // The codec (`read_info`), filled for existing samples by the next
        // catch-up, which reads every file again.
        db.execute_batch(
            "ALTER TABLE files ADD COLUMN codec TEXT;
             PRAGMA user_version = 9;",
        )?;
    }
    if version < 10 {
        // The kind of sound set by hand: `family` or `family/subtype` of
        // `sound_type::FAMILIES`, '' for none; NULL: worked out from the
        // path.
        db.execute_batch(
            "ALTER TABLE files ADD COLUMN sound_type TEXT;
             PRAGMA user_version = 10;",
        )?;
    }
    if version < 11 {
        // The tempo detected from the sound (see `pulse`), filled as the
        // waveforms are computed again (`MEASURED`).
        db.execute_batch(
            "ALTER TABLE waveforms ADD COLUMN detected_bpm INTEGER;
             PRAGMA user_version = 11;",
        )?;
    }
    if version < 12 {
        // How a tempo stored in `files.bpm` was found: NULL typed by hand,
        // else the method chosen in "Detect BPM": 'name', 'file', 'length'
        // or 'sound'.
        db.execute_batch(
            "ALTER TABLE files ADD COLUMN bpm_origin TEXT;
             PRAGMA user_version = 12;",
        )?;
    }
    db.execute_batch("COMMIT")?;
    Ok(())
}

/// Remembers a scanned folder, so that `samplr watch` follows it.
pub fn add_folder(db: &Connection, path: &Path) -> Result<(), Box<dyn Error>> {
    db.execute(
        "INSERT INTO folders (path) VALUES (?1) ON CONFLICT (path) DO NOTHING",
        [path_as_text(path)?],
    )?;
    Ok(())
}

/// The folders scanned so far, in alphabetical order.
pub fn folders(db: &Connection) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut query = db.prepare("SELECT path FROM folders ORDER BY path")?;
    let rows = query.query_map([], |row| Ok(PathBuf::from(row.get::<_, String>(0)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Removes a folder from the library: it is no longer followed, and its
/// samples are hidden, unless another scanned folder still contains them.
/// Their tags and info are kept, in case the folder is added again; their
/// waveforms are deleted. Returns the number of samples hidden.
pub fn remove_folder(db: &Connection, folder: &Path) -> Result<usize, Box<dyn Error>> {
    let removed = db.execute(
        "DELETE FROM folders WHERE path = ?1",
        [path_as_text(folder)?],
    )?;
    if removed == 0 {
        return Err(format!("{} is not a scanned folder", folder.display()).into());
    }
    let others = folders(db)?;

    let mut hidden = 0;
    for path in files_under(db, folder)? {
        if others.iter().any(|other| path.starts_with(other)) {
            continue;
        }
        let text = path_as_text(&path)?;
        db.execute("UPDATE files SET hidden = 1 WHERE path = ?1", [text])?;
        db.execute(
            "DELETE FROM waveforms WHERE file_id = (SELECT id FROM files WHERE path = ?1)",
            [text],
        )?;
        hidden += 1;
    }
    Ok(hidden)
}

/// The number of samples shown (not hidden) in `folder` or its subfolders.
pub fn count_under(db: &Connection, folder: &Path) -> Result<u32, Box<dyn Error>> {
    let folder = path_as_text(folder)?;
    let count = db.query_row(
        &format!("SELECT COUNT(*) FROM files WHERE hidden = 0 AND {UNDER_PATH}"),
        named_params! { ":path": folder, ":prefix": folder_prefix(folder) },
        |row| row.get(0),
    )?;
    Ok(count)
}

/// The samples in the database located in `folder` or its subfolders.
pub fn files_under(db: &Connection, folder: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let folder = path_as_text(folder)?;
    let mut query = db.prepare(&format!("SELECT path FROM files WHERE {UNDER_PATH}"))?;
    let rows = query.query_map(
        named_params! { ":path": folder, ":prefix": folder_prefix(folder) },
        |row| Ok(PathBuf::from(row.get::<_, String>(0)?)),
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Removes the sample at `path` from the database, or every sample it
/// contains if it is a folder, along with their tags. Returns the number of
/// samples removed.
pub fn forget(db: &Connection, path: &Path) -> Result<usize, Box<dyn Error>> {
    let path = path_as_text(path)?;
    let forgotten = db.execute(
        &format!("DELETE FROM files WHERE {UNDER_PATH}"),
        named_params! { ":path": path, ":prefix": folder_prefix(path) },
    )?;
    if forgotten > 0 {
        delete_orphan_tags(db)?;
    }
    Ok(forgotten)
}

/// Applies a rename or a move to the database: the sample (or every sample in
/// the folder) changes path but keeps its `id`, hence its tags. Returns the
/// number of samples renamed.
pub fn rename(db: &Connection, old: &Path, new: &Path) -> Result<usize, Box<dyn Error>> {
    let old = path_as_text(old)?;
    // Nothing recorded at the old path: nothing to rename. This is the case
    // when the move was already applied (`move_sample`, then the watcher
    // reporting the same rename) or for a file coming from outside (a
    // converted copy renamed from its `.part`): the row already at the new
    // path is the right one, and must not be forgotten below.
    let recorded: u32 = db.query_row(
        &format!("SELECT COUNT(*) FROM files WHERE {UNDER_PATH}"),
        named_params! { ":path": old, ":prefix": folder_prefix(old) },
        |row| row.get(0),
    )?;
    if recorded == 0 {
        return Ok(0);
    }
    // If a sample already sat at the new path (overwritten file), it is
    // replaced: otherwise the UNIQUE constraint would make the update fail.
    forget(db, new)?;
    // Replaces the start of the path: `/a/kick.wav` becomes `/b/kick.wav`,
    // `/a/drums/kick.wav` becomes `/b/drums/kick.wav`.
    let renamed = db.execute(
        &format!(
            "UPDATE files SET path = :new || substr(path, length(:path) + 1) WHERE {UNDER_PATH}"
        ),
        named_params! {
            ":path": old,
            ":prefix": folder_prefix(old),
            ":new": path_as_text(new)?,
        },
    )?;
    Ok(renamed)
}

/// "The `path` column is `:path`, or starts with `:prefix`", where `:prefix`
/// is `:path` seen as a folder (see `folder_prefix`). No `LIKE`: a `_` or a
/// `%` in a folder name would act as a wildcard.
const UNDER_PATH: &str = "(path = :path OR substr(path, 1, length(:prefix)) = :prefix)";

/// `path` followed by the system's separator (`/` on Mac and Linux, `\` on
/// Windows): what every path inside that folder starts with. Without the
/// separator, `/samples/kick` would also match `/samples/kicks/…`. A drive
/// root (`/`, `C:\`) already ends with one.
fn folder_prefix(path: &str) -> String {
    if path.ends_with(MAIN_SEPARATOR) {
        path.to_string()
    } else {
        format!("{path}{MAIN_SEPARATOR}")
    }
}

/// Adds a sample to the database, or updates its info if it is already there.
/// Returns the sample's `id`.
pub fn save(db: &Connection, info: &SampleInfo) -> Result<i64, Box<dyn Error>> {
    let path = path_as_text(&info.path)?;

    // `ON CONFLICT … DO UPDATE` updates the existing row instead of replacing
    // it: the sample keeps its `id`, hence its tags. `RETURNING id` gives
    // that `id` in both cases, insert or update. A sample hidden with its
    // folder shows again when found again.
    let id = db.query_row(
        "INSERT INTO files (path, duration_secs, sample_rate, channels, bits,
                            acid_beats, acid_root, acid_loop, codec)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (path) DO UPDATE SET
             duration_secs = excluded.duration_secs,
             sample_rate   = excluded.sample_rate,
             channels      = excluded.channels,
             bits          = excluded.bits,
             codec         = excluded.codec,
             acid_beats    = excluded.acid_beats,
             acid_root     = excluded.acid_root,
             acid_loop     = excluded.acid_loop,
             hidden        = 0
         RETURNING id",
        params![
            path,
            info.duration_secs,
            info.sample_rate,
            info.channels,
            info.bits,
            info.acid.as_ref().map(|a| a.beats),
            info.acid.as_ref().and_then(|a| a.root.clone()),
            info.acid.as_ref().map(|a| a.looped),
            info.codec,
        ],
        |row| row.get(0),
    )?;
    Ok(id)
}

/// Every sample shown (not hidden): its `id` and its path, sorted by path.
pub fn all_files(db: &Connection) -> Result<Vec<(i64, PathBuf)>, Box<dyn Error>> {
    let mut query = db.prepare("SELECT id, path FROM files WHERE hidden = 0 ORDER BY path")?;
    let rows = query.query_map([], |row| {
        Ok((row.get(0)?, PathBuf::from(row.get::<_, String>(1)?)))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The cached waveform of a sample, or `None` if there is none or if it is
/// stale. The file's size and modification date are saved with the
/// waveform: if either has changed, the file has changed and the waveform
/// must be computed again.
pub fn waveform(
    db: &Connection,
    file_id: i64,
    stamp: FileStamp,
) -> Result<Option<Vec<Peak>>, Box<dyn Error>> {
    let bytes: Option<Vec<u8>> = db
        .query_row(
            "SELECT peaks FROM waveforms
             WHERE file_id = ?1 AND size = ?2 AND modified = ?3 AND measured = ?4",
            params![file_id, stamp.size, stamp.modified, MEASURED],
            |row| row.get(0),
        )
        .optional()?;
    Ok(bytes.map(|b| waveform::from_bytes(&b)))
}

/// Whether the cached waveform of a sample is up to date, without reading
/// its peaks.
pub fn has_waveform(
    db: &Connection,
    file_id: i64,
    stamp: FileStamp,
) -> Result<bool, Box<dyn Error>> {
    let found = db
        .query_row(
            "SELECT 1 FROM waveforms
             WHERE file_id = ?1 AND size = ?2 AND modified = ?3 AND measured = ?4",
            params![file_id, stamp.size, stamp.modified, MEASURED],
            |_| Ok(()),
        )
        .optional()?;
    Ok(found.is_some())
}

/// Saves a sample's waveform, replacing the previous one.
pub fn save_waveform(
    db: &Connection,
    file_id: i64,
    stamp: FileStamp,
    peaks: &[Peak],
    measures: waveform::Measures,
) -> Result<(), Box<dyn Error>> {
    db.execute(
        "INSERT OR REPLACE INTO waveforms
             (file_id, size, modified, peaks, lufs, peak_db, detected_bpm, measured)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            file_id,
            stamp.size,
            stamp.modified,
            waveform::to_bytes(peaks),
            measures.lufs,
            measures.peak_db,
            measures.tempo,
            MEASURED,
        ],
    )?;
    Ok(())
}

/// What tells whether a file has changed since a waveform was computed:
/// its size in bytes and its modification date (milliseconds since 1970).
/// `#[derive(Clone, Copy)]` lets it be passed around by value, like a number.
#[derive(Clone, Copy)]
pub struct FileStamp {
    pub size: i64,
    pub modified: i64,
}

impl FileStamp {
    /// Reads the stamp of the file at `path` from the file system.
    pub fn read(path: &Path) -> Result<FileStamp, Box<dyn Error>> {
        let metadata = fs::metadata(path)?;
        let modified = metadata.modified()?.duration_since(UNIX_EPOCH)?;
        Ok(FileStamp {
            size: metadata.len() as i64,
            modified: modified.as_millis() as i64,
        })
    }
}

/// A sample's `id` from its path, or `None` if it is not in the database.
pub fn file_id(db: &Connection, path: &Path) -> Result<Option<i64>, Box<dyn Error>> {
    let path = path_as_text(path)?;
    // `optional()` turns "no row found", which would be an error for
    // `query_row`, into a plain `None`.
    let id = db
        .query_row("SELECT id FROM files WHERE path = ?1", [path], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(id)
}

/// The `id` of a sample, an error when it is not in the database.
pub fn require_file_id(db: &Connection, path: &Path) -> Result<i64, Box<dyn Error>> {
    file_id(db, path)?.ok_or_else(|| format!("{} is not in the database", path.display()).into())
}

/// Adds tags to a sample. Returns the number of tags actually added (a tag
/// the sample already has does not count).
pub fn add_tags(db: &Connection, file_id: i64, tags: &[String]) -> Result<usize, Box<dyn Error>> {
    let mut added = 0;
    for tag in tags {
        let tag = normalize_tag(tag)?;
        db.execute(
            "INSERT INTO tags (name) VALUES (?1) ON CONFLICT (name) DO NOTHING",
            [&tag],
        )?;
        // `execute` returns the number of rows affected: 0 if the link
        // already existed (`OR IGNORE`).
        added += db.execute(
            "INSERT OR IGNORE INTO file_tags (file_id, tag_id)
             SELECT ?1, id FROM tags WHERE name = ?2",
            params![file_id, tag],
        )?;
    }
    Ok(added)
}

/// Removes tags from a sample. Returns the number of tags actually removed.
pub fn remove_tags(
    db: &Connection,
    file_id: i64,
    tags: &[String],
) -> Result<usize, Box<dyn Error>> {
    let mut removed = 0;
    for tag in tags {
        let tag = normalize_tag(tag)?;
        removed += db.execute(
            "DELETE FROM file_tags
             WHERE file_id = ?1
               AND tag_id = (SELECT id FROM tags WHERE name = ?2)",
            params![file_id, tag],
        )?;
    }
    delete_orphan_tags(db)?;
    Ok(removed)
}

/// A tag no longer used by any sample disappears from the tag list.
fn delete_orphan_tags(db: &Connection) -> Result<(), Box<dyn Error>> {
    db.execute(
        "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM file_tags)",
        [],
    )?;
    Ok(())
}

/// A colour of the palette: `hex` is `#rrggbb`.
#[derive(Serialize)]
pub struct Color {
    pub id: i64,
    pub name: String,
    pub hex: String,
}

/// The palette, in its order.
pub fn colors(db: &Connection) -> Result<Vec<Color>, Box<dyn Error>> {
    let mut query = db.prepare("SELECT id, name, hex FROM colors ORDER BY position, id")?;
    let rows = query.query_map([], |row| {
        Ok(Color {
            id: row.get(0)?,
            name: row.get(1)?,
            hex: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// `#rrggbb`, lowercase, as an `<input type="color">` gives it.
fn check_hex(hex: &str) -> Result<String, Box<dyn Error>> {
    let hex = hex.trim().to_lowercase();
    let digits = hex.strip_prefix('#').unwrap_or("");
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("invalid colour: \"{hex}\" (expected #rrggbb)").into());
    }
    Ok(hex)
}

/// Adds a colour at the end of the palette and returns its `id`.
pub fn add_color(db: &Connection, name: &str, hex: &str) -> Result<i64, Box<dyn Error>> {
    let id = db.query_row(
        "INSERT INTO colors (name, hex, position)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM colors))
         RETURNING id",
        params![name.trim(), check_hex(hex)?],
        |row| row.get(0),
    )?;
    Ok(id)
}

/// Renames a colour or changes its value.
pub fn update_color(db: &Connection, id: i64, name: &str, hex: &str) -> Result<(), Box<dyn Error>> {
    db.execute(
        "UPDATE colors SET name = ?1, hex = ?2 WHERE id = ?3",
        params![name.trim(), check_hex(hex)?, id],
    )?;
    Ok(())
}

/// How many tags and how many collections use a colour.
pub fn color_usage(db: &Connection, id: i64) -> Result<(u32, u32), Box<dyn Error>> {
    let usage = db.query_row(
        "SELECT (SELECT COUNT(*) FROM tags WHERE color_id = ?1),
                (SELECT COUNT(*) FROM collections WHERE color_id = ?1)",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(usage)
}

/// Deletes a colour; its tags and collections are left without colour.
pub fn delete_color(db: &Connection, id: i64) -> Result<(), Box<dyn Error>> {
    db.execute("DELETE FROM colors WHERE id = ?1", [id])?;
    Ok(())
}

/// The tags that have a colour: their name and their colour's `id`.
pub fn tag_colors(db: &Connection) -> Result<Vec<(String, i64)>, Box<dyn Error>> {
    let mut query = db.prepare("SELECT name, color_id FROM tags WHERE color_id IS NOT NULL")?;
    let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Gives a tag a colour, or none (`None`).
pub fn set_tag_color(
    db: &Connection,
    tag: &str,
    color_id: Option<i64>,
) -> Result<(), Box<dyn Error>> {
    db.execute(
        "UPDATE tags SET color_id = ?1 WHERE name = ?2",
        params![color_id, normalize_tag(tag)?],
    )?;
    Ok(())
}

/// Gives a collection a colour, or none (`None`).
pub fn set_collection_color(
    db: &Connection,
    collection_id: i64,
    color_id: Option<i64>,
) -> Result<(), Box<dyn Error>> {
    db.execute(
        "UPDATE collections SET color_id = ?1 WHERE id = ?2",
        params![color_id, collection_id],
    )?;
    Ok(())
}

/// Marks a sample as a favorite, or no longer.
pub fn set_favorite(db: &Connection, file_id: i64, favorite: bool) -> Result<(), Box<dyn Error>> {
    db.execute(
        "UPDATE files SET favorite = ?1 WHERE id = ?2",
        params![favorite, file_id],
    )?;
    Ok(())
}

/// Sets a sample's tempo by hand: `Some(0)` for none, `None` to go back to
/// the one worked out from the name or the length.
pub fn set_tempo(db: &Connection, file_id: i64, bpm: Option<u32>) -> Result<(), Box<dyn Error>> {
    db.execute(
        "UPDATE files SET bpm = ?1, bpm_origin = NULL WHERE id = ?2",
        params![bpm, file_id],
    )?;
    Ok(())
}

/// Sets a sample's tempo to the one a method found on request ("Detect
/// BPM"), over any other, even one typed by hand. `origin`: the method,
/// "name", "file", "length" or "sound".
pub fn set_detected_tempo(
    db: &Connection,
    file_id: i64,
    bpm: u32,
    origin: &str,
) -> Result<(), Box<dyn Error>> {
    db.execute(
        "UPDATE files SET bpm = ?1, bpm_origin = ?2 WHERE id = ?3",
        params![bpm, origin, file_id],
    )?;
    Ok(())
}

/// Sets a sample's key by hand, in sharps ("A#m"): `Some("")` for none,
/// `None` to go back to the one read in the name.
pub fn set_key(db: &Connection, file_id: i64, key: Option<&str>) -> Result<(), Box<dyn Error>> {
    db.execute(
        "UPDATE files SET key = ?1 WHERE id = ?2",
        params![key, file_id],
    )?;
    Ok(())
}

/// Sets a sample as a loop or a one-shot by hand; `None` goes back to the
/// value worked out.
pub fn set_loop(db: &Connection, file_id: i64, looped: Option<bool>) -> Result<(), Box<dyn Error>> {
    db.execute(
        "UPDATE files SET looped = ?1 WHERE id = ?2",
        params![looped, file_id],
    )?;
    Ok(())
}

/// Sets a sample's kind of sound by hand: `Some("")` for none, `None` to go
/// back to the one worked out from the path.
pub fn set_sound_type(
    db: &Connection,
    file_id: i64,
    sound_type: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    db.execute(
        "UPDATE files SET sound_type = ?1 WHERE id = ?2",
        params![sound_type, file_id],
    )?;
    Ok(())
}

/// A collection: samples gathered by hand. A sample can be in several.
#[derive(Serialize)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    /// Samples shown (not hidden) in the collection.
    pub samples: u32,
    /// Its colour in the palette, if it has one.
    pub color_id: Option<i64>,
}

/// The collections, sorted by name (case-insensitive).
pub fn collections(db: &Connection) -> Result<Vec<Collection>, Box<dyn Error>> {
    let mut query = db.prepare(
        "SELECT c.id, c.name,
                (SELECT COUNT(*) FROM collection_files cf JOIN files f ON f.id = cf.file_id
                 WHERE cf.collection_id = c.id AND f.hidden = 0),
                c.color_id
         FROM collections c
         ORDER BY c.name COLLATE NOCASE",
    )?;
    let rows = query.query_map([], |row| {
        Ok(Collection {
            id: row.get(0)?,
            name: row.get(1)?,
            samples: row.get(2)?,
            color_id: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Creates an empty collection and returns its `id`.
pub fn create_collection(db: &Connection, name: &str) -> Result<i64, Box<dyn Error>> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a collection needs a name".into());
    }
    let id = db
        .query_row(
            "INSERT INTO collections (name) VALUES (?1)
             ON CONFLICT (name) DO NOTHING
             RETURNING id",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    id.ok_or_else(|| format!("a collection named \"{name}\" already exists").into())
}

/// Deletes a collection. The samples are not touched.
pub fn delete_collection(db: &Connection, collection_id: i64) -> Result<(), Box<dyn Error>> {
    db.execute("DELETE FROM collections WHERE id = ?1", [collection_id])?;
    Ok(())
}

/// Adds samples to a collection; those already in it are left as they are.
/// Returns the number added.
pub fn add_to_collection(
    db: &Connection,
    collection_id: i64,
    file_ids: &[i64],
) -> Result<usize, Box<dyn Error>> {
    let mut added = 0;
    for file_id in file_ids {
        added += db.execute(
            "INSERT OR IGNORE INTO collection_files (collection_id, file_id) VALUES (?1, ?2)",
            [collection_id, *file_id],
        )?;
    }
    Ok(added)
}

/// The `id`s of the collections that already hold every one of `file_ids`.
pub fn collections_holding(db: &Connection, file_ids: &[i64]) -> Result<Vec<i64>, Box<dyn Error>> {
    // One `?` per sample, and SQLite takes at most 32766 of them in a query:
    // a big selection is asked by parts, and only the collections holding
    // every part are kept.
    let mut holding: Option<Vec<i64>> = None;
    for part in file_ids.chunks(10_000) {
        let marks = vec!["?"; part.len()].join(", ");
        let mut query = db.prepare(&format!(
            "SELECT collection_id FROM collection_files
             WHERE file_id IN ({marks})
             GROUP BY collection_id
             HAVING COUNT(*) = {}",
            part.len()
        ))?;
        let ids: Vec<i64> = query
            .query_map(params_from_iter(part), |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        holding = Some(match holding {
            None => ids,
            Some(before) => before.into_iter().filter(|id| ids.contains(id)).collect(),
        });
    }
    Ok(holding.unwrap_or_default())
}

/// Takes samples out of a collection (only the links: the samples stay in
/// the library). Returns the number removed.
pub fn remove_from_collection(
    db: &Connection,
    collection_id: i64,
    file_ids: &[i64],
) -> Result<usize, Box<dyn Error>> {
    let mut removed = 0;
    for file_id in file_ids {
        removed += db.execute(
            "DELETE FROM collection_files WHERE collection_id = ?1 AND file_id = ?2",
            [collection_id, *file_id],
        )?;
    }
    Ok(removed)
}

/// A sample's tags, in alphabetical order.
pub fn tags_of(db: &Connection, file_id: i64) -> Result<Vec<String>, Box<dyn Error>> {
    let mut query = db.prepare(
        "SELECT t.name
         FROM tags t JOIN file_tags ft ON ft.tag_id = t.id
         WHERE ft.file_id = ?1
         ORDER BY t.name",
    )?;
    let rows = query.query_map([file_id], |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Every tag carried by a shown sample (not hidden), with the number of such
/// samples, in alphabetical order.
pub fn list_tags(db: &Connection) -> Result<Vec<(String, u32)>, Box<dyn Error>> {
    let mut query = db.prepare(
        "SELECT t.name, COUNT(*)
         FROM tags t JOIN file_tags ft ON ft.tag_id = t.id
                     JOIN files f ON f.id = ft.file_id AND f.hidden = 0
         GROUP BY t.id
         ORDER BY t.name",
    )?;
    let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The samples carrying **all** the given `tags`, and where each of the
/// `words` is found in the path or in one of the tags (case-insensitive),
/// sorted by path. With no tag and no word, every sample shown. Hidden samples (see `remove_folder`) are left
/// out. `favorites_only`: only the favorites. `collection`: only the samples
/// of that collection.
pub fn search(
    db: &Connection,
    tags: &[String],
    words: &[&str],
    favorites_only: bool,
    collection: Option<i64>,
) -> Result<Vec<SampleInfo>, Box<dyn Error>> {
    let mut sql = String::from(
        "SELECT f.path, f.duration_secs, f.sample_rate, f.channels, f.bits,
                (SELECT group_concat(t.name, ' ' ORDER BY t.name)
                 FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
                 WHERE ft.file_id = f.id),
                f.favorite,
                (SELECT group_concat(cf.collection_id) FROM collection_files cf
                 WHERE cf.file_id = f.id),
                f.bpm, f.key, f.acid_beats, f.acid_root, f.acid_loop, f.looped,
                w.lufs, w.peak_db, f.codec, f.sound_type, w.detected_bpm, f.bpm_origin
         FROM files f
         LEFT JOIN waveforms w ON w.file_id = f.id AND w.measured >= 1
         WHERE f.hidden = 0",
    );
    if favorites_only {
        sql.push_str(" AND f.favorite = 1");
    }
    let mut values = Vec::new();
    if let Some(collection) = collection {
        // Bound as text like the other values: compared with an INTEGER
        // column, SQLite turns it back into a number.
        values.push(collection.to_string());
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM collection_files cf
                          WHERE cf.file_id = f.id AND cf.collection_id = ?{})",
            values.len()
        ));
    }

    // The number of tags varies: one condition, and one `?N`, per tag.
    for tag in tags {
        values.push(normalize_tag(tag)?);
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
                          WHERE ft.file_id = f.id AND t.name = ?{})",
            values.len()
        ));
    }
    // `instr` looks for the word as is: with `LIKE`, a `_` (common in sample
    // names) would act as a wildcard. Tags are stored lowercase: only the
    // path needs `unicode_lower`; the word is lowercased here, by the same
    // Rust `to_lowercase`, once per search rather than once per row.
    for word in words {
        values.push(word.to_lowercase());
        let n = values.len();
        sql.push_str(&format!(
            " AND (instr(unicode_lower(f.path), ?{n}) > 0
                   OR EXISTS (SELECT 1 FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
                              WHERE ft.file_id = f.id AND instr(t.name, ?{n}) > 0))"
        ));
    }
    sql.push_str(" ORDER BY f.path");

    let mut query = db.prepare(&sql)?;
    let rows = query.query_map(params_from_iter(&values), |row| {
        let path = PathBuf::from(row.get::<_, String>(0)?);
        let tags: Option<String> = row.get(5)?;
        let duration_secs: Option<f64> = row.get(1)?;
        // Worked out from the name, the ACID chunk and the length at each
        // search, rather than stored: a renamed file needs nothing more.
        // What was set by hand comes first, then the name, then the chunk.
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let acid_beats: Option<u32> = row.get(10)?;
        let acid_root: Option<String> = row.get(11)?;
        let acid_loop: Option<bool> = row.get(12)?;
        let stored_source = row
            .get::<_, Option<String>>(19)?
            .as_deref()
            .and_then(analysis::Source::from_name)
            .unwrap_or(analysis::Source::Manual);
        let stored_tempo: Option<u32> = row.get(8)?;
        let tempo = match stored_tempo {
            Some(0) => None,
            Some(bpm) => Some((bpm, stored_source)),
            None => analysis::tempo(stem, duration_secs, acid_beats),
        };
        let key = match row.get::<_, Option<String>>(9)? {
            Some(key) if key.is_empty() => None,
            Some(key) => Some(key),
            None => analysis::key_from_name(stem).or(acid_root.clone()),
        };
        let (looped, loop_source) = match row.get::<_, Option<bool>>(13)? {
            Some(looped) => (looped, analysis::Source::Manual),
            None => analysis::looped(&path, acid_loop, tempo.is_some()),
        };
        // Last resort, for a loop only: the tempo detected from the sound,
        // better than the one from the length alone (see `pulse`). A one-shot
        // keeps none, and the detection does not make it a loop.
        let detected: Option<u32> = row.get(18)?;
        let tempo = match (tempo, detected) {
            (None | Some((_, analysis::Source::Length)), Some(bpm))
                if looped && stored_tempo.is_none() =>
            {
                Some((bpm, analysis::Source::Sound))
            }
            (tempo, _) => tempo,
        };
        // A value no longer in the list counts as not set.
        let chosen = row
            .get::<_, Option<String>>(17)?
            .filter(|chosen| chosen.is_empty() || sound_type::parse(chosen).is_some());
        let (sound_type, type_source) = match chosen {
            Some(chosen) if chosen.is_empty() => (None, Some(analysis::Source::Manual)),
            Some(chosen) => (Some(chosen), Some(analysis::Source::Manual)),
            None => match sound_type::from_path(&path) {
                Some(found) => (Some(found), Some(analysis::Source::Name)),
                None => (None, None),
            },
        };
        Ok(SampleInfo {
            duration_secs,
            sample_rate: row.get(2)?,
            channels: row.get(3)?,
            bits: row.get(4)?,
            tags: tags
                .map(|t| t.split(' ').map(String::from).collect())
                .unwrap_or_default(),
            favorite: row.get(6)?,
            collections: row
                .get::<_, Option<String>>(7)?
                .map(|ids| ids.split(',').filter_map(|id| id.parse().ok()).collect())
                .unwrap_or_default(),
            bpm: tempo.map(|(bpm, _)| bpm),
            bpm_source: tempo.map(|(_, source)| source),
            bpm_fixed: stored_tempo.is_some_and(|bpm| bpm > 0)
                && stored_source != analysis::Source::Manual,
            key,
            looped,
            loop_source: Some(loop_source),
            sound_type,
            type_source,
            lufs: row.get(14)?,
            peak_db: row.get(15)?,
            codec: row.get(16)?,
            acid: acid_loop.map(|looped| acid::Acid {
                beats: acid_beats.unwrap_or(0),
                root: acid_root,
                looped,
            }),
            path,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// A tag is stored lowercase with no surrounding spaces: `Kick` and ` kick`
/// are the same tag. Spaces inside are rejected, they separate tags on display.
fn normalize_tag(tag: &str) -> Result<String, Box<dyn Error>> {
    let tag = tag.trim().to_lowercase();
    if tag.is_empty() || tag.contains(char::is_whitespace) {
        return Err(format!("invalid tag: \"{tag}\" (empty or containing spaces)").into());
    }
    Ok(tag)
}

/// Paths are stored as text: a file name that is not valid UTF-8 (possible
/// on Linux) could not be found again.
fn path_as_text(path: &Path) -> Result<&str, Box<dyn Error>> {
    Ok(path.to_str().ok_or("file name with invalid characters")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new database in memory, at the current schema.
    fn new_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        set_up(&db).unwrap();
        db
    }

    /// Saves a sample as `scan` would, with nothing read from a file.
    fn add(db: &Connection, path: &str) -> i64 {
        let info = SampleInfo {
            path: PathBuf::from(path),
            duration_secs: Some(1.0),
            sample_rate: Some(44_100),
            channels: Some(2),
            bits: Some(16),
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
            codec: None,
            acid: None,
        };
        save(db, &info).unwrap()
    }

    fn tag(db: &Connection, file_id: i64, tags: &[&str]) {
        let tags: Vec<String> = tags.iter().map(|t| t.to_string()).collect();
        add_tags(db, file_id, &tags).unwrap();
    }

    /// The paths a search finds, by words typed in the search box.
    fn found(db: &Connection, words: &[&str]) -> Vec<String> {
        search(db, &[], words, false, None)
            .unwrap()
            .into_iter()
            .map(|sample| sample.path.display().to_string())
            .collect()
    }

    #[test]
    fn new_database_reaches_the_current_version() {
        let db = new_db();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(colors(&db).unwrap().len(), 10);
        // Opening it again changes nothing: no step runs twice.
        set_up(&db).unwrap();
        assert_eq!(colors(&db).unwrap().len(), 10);
    }

    #[test]
    fn a_new_scan_keeps_the_id_and_the_tags() {
        let db = new_db();
        let id = add(&db, "/lib/kick.wav");
        tag(&db, id, &["drums"]);
        assert_eq!(add(&db, "/lib/kick.wav"), id);
        assert_eq!(tags_of(&db, id).unwrap(), ["drums"]);
    }

    #[test]
    fn tags_are_stored_lowercase_and_trimmed() {
        let db = new_db();
        let id = add(&db, "/lib/kick.wav");
        tag(&db, id, &["Kick", " kick ", "KICK"]);
        assert_eq!(tags_of(&db, id).unwrap(), ["kick"]);
        let spaced = add_tags(&db, id, &["two words".to_string()]);
        assert!(spaced.is_err());
        assert!(add_tags(&db, id, &[" ".to_string()]).is_err());
    }

    #[test]
    fn a_tag_no_sample_carries_is_deleted() {
        let db = new_db();
        let id = add(&db, "/lib/kick.wav");
        tag(&db, id, &["drums", "dry"]);
        let removed = remove_tags(&db, id, &["DRUMS".to_string()]).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(list_tags(&db).unwrap(), [("dry".to_string(), 1)]);
    }

    #[test]
    fn removing_a_folder_hides_its_samples_and_keeps_their_tags() {
        let db = new_db();
        add_folder(&db, Path::new("/lib")).unwrap();
        let id = add(&db, "/lib/kick.wav");
        tag(&db, id, &["drums"]);

        assert_eq!(remove_folder(&db, Path::new("/lib")).unwrap(), 1);
        assert!(found(&db, &[]).is_empty());
        assert!(list_tags(&db).unwrap().is_empty());

        // Scanned again: back, with the same id and its tags.
        add_folder(&db, Path::new("/lib")).unwrap();
        assert_eq!(add(&db, "/lib/kick.wav"), id);
        assert_eq!(found(&db, &[]), ["/lib/kick.wav"]);
        assert_eq!(tags_of(&db, id).unwrap(), ["drums"]);
    }

    #[test]
    fn a_sample_inside_another_scanned_folder_stays_shown() {
        let db = new_db();
        add_folder(&db, Path::new("/lib")).unwrap();
        add_folder(&db, Path::new("/lib/drums")).unwrap();
        add(&db, "/lib/drums/kick.wav");
        add(&db, "/lib/pad.wav");
        assert_eq!(remove_folder(&db, Path::new("/lib")).unwrap(), 1);
        assert_eq!(found(&db, &[]), ["/lib/drums/kick.wav"]);
    }

    #[test]
    fn removing_a_folder_not_scanned_is_refused() {
        let db = new_db();
        assert!(remove_folder(&db, Path::new("/elsewhere")).is_err());
    }

    #[test]
    fn a_folder_does_not_hold_its_namesakes() {
        let db = new_db();
        add(&db, "/lib/kick/a.wav");
        add(&db, "/lib/kicks/b.wav");
        assert_eq!(count_under(&db, Path::new("/lib/kick")).unwrap(), 1);
        assert_eq!(forget(&db, Path::new("/lib/kick")).unwrap(), 1);
        assert_eq!(found(&db, &[]), ["/lib/kicks/b.wav"]);
    }

    #[test]
    fn forgetting_a_sample_deletes_its_last_tags() {
        let db = new_db();
        let id = add(&db, "/lib/kick.wav");
        tag(&db, id, &["drums"]);
        assert_eq!(forget(&db, Path::new("/lib/kick.wav")).unwrap(), 1);
        assert!(list_tags(&db).unwrap().is_empty());
    }

    #[test]
    fn renaming_a_folder_keeps_the_ids_and_tags() {
        let db = new_db();
        let kick = add(&db, "/lib/drums/kick.wav");
        let snare = add(&db, "/lib/drums/deep/snare.wav");
        tag(&db, kick, &["drums"]);

        let renamed = rename(&db, Path::new("/lib/drums"), Path::new("/lib/perc")).unwrap();
        assert_eq!(renamed, 2);
        assert_eq!(
            file_id(&db, Path::new("/lib/perc/kick.wav")).unwrap(),
            Some(kick)
        );
        assert_eq!(
            file_id(&db, Path::new("/lib/perc/deep/snare.wav")).unwrap(),
            Some(snare)
        );
        assert_eq!(tags_of(&db, kick).unwrap(), ["drums"]);
    }

    #[test]
    fn a_rename_already_applied_changes_nothing() {
        let db = new_db();
        let id = add(&db, "/lib/new.wav");
        tag(&db, id, &["drums"]);
        // The watcher reporting a move `move_sample` already applied.
        let renamed = rename(&db, Path::new("/lib/old.wav"), Path::new("/lib/new.wav")).unwrap();
        assert_eq!(renamed, 0);
        assert_eq!(tags_of(&db, id).unwrap(), ["drums"]);
    }

    #[test]
    fn a_rename_over_a_sample_replaces_it() {
        let db = new_db();
        let kept = add(&db, "/lib/a.wav");
        add(&db, "/lib/b.wav");
        rename(&db, Path::new("/lib/a.wav"), Path::new("/lib/b.wav")).unwrap();
        assert_eq!(file_id(&db, Path::new("/lib/b.wav")).unwrap(), Some(kept));
        assert_eq!(found(&db, &[]), ["/lib/b.wav"]);
    }

    #[test]
    fn search_words_match_the_path_or_a_tag() {
        let db = new_db();
        let kick = add(&db, "/lib/Kick_01.wav");
        add(&db, "/lib/Snare_01.wav");
        tag(&db, kick, &["punchy"]);
        assert_eq!(found(&db, &["kick"]), ["/lib/Kick_01.wav"]);
        assert_eq!(found(&db, &["PUNCH"]), ["/lib/Kick_01.wav"]);
        // Every word must be found.
        assert!(found(&db, &["kick", "snare"]).is_empty());
    }

    #[test]
    fn search_folds_case_beyond_ascii() {
        let db = new_db();
        add(&db, "/lib/Été/pad.wav");
        assert_eq!(found(&db, &["été"]), ["/lib/Été/pad.wav"]);
        assert_eq!(found(&db, &["ÉTÉ"]), ["/lib/Été/pad.wav"]);
    }

    #[test]
    fn search_takes_an_underscore_literally() {
        let db = new_db();
        add(&db, "/lib/kick_1.wav");
        add(&db, "/lib/kickA1.wav");
        // As a `LIKE` wildcard, `_` would also match the `A`.
        assert_eq!(found(&db, &["k_1"]), ["/lib/kick_1.wav"]);
    }

    #[test]
    fn search_by_tags_needs_all_of_them() {
        let db = new_db();
        let both = add(&db, "/lib/a.wav");
        let one = add(&db, "/lib/b.wav");
        tag(&db, both, &["drums", "dry"]);
        tag(&db, one, &["drums"]);
        let tags = ["Drums".to_string(), "dry".to_string()];
        let samples = search(&db, &tags, &[], false, None).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].path, Path::new("/lib/a.wav"));
        assert_eq!(samples[0].tags, ["drums", "dry"]);
    }

    #[test]
    fn search_filters_favorites_and_collections() {
        let db = new_db();
        let a = add(&db, "/lib/a.wav");
        let b = add(&db, "/lib/b.wav");
        set_favorite(&db, a, true).unwrap();
        let collection = create_collection(&db, "Set").unwrap();
        add_to_collection(&db, collection, &[b]).unwrap();

        let favorites = search(&db, &[], &[], true, None).unwrap();
        assert_eq!(favorites.len(), 1);
        assert!(favorites[0].favorite);
        let held = search(&db, &[], &[], false, Some(collection)).unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].path, Path::new("/lib/b.wav"));
        assert_eq!(held[0].collections, [collection]);
    }

    #[test]
    fn collections_holding_keeps_those_holding_every_sample() {
        let db = new_db();
        let a = add(&db, "/lib/a.wav");
        let b = add(&db, "/lib/b.wav");
        let both = create_collection(&db, "Both").unwrap();
        let only_a = create_collection(&db, "Only a").unwrap();
        add_to_collection(&db, both, &[a, b]).unwrap();
        add_to_collection(&db, only_a, &[a]).unwrap();
        // Adding twice adds nothing.
        assert_eq!(add_to_collection(&db, both, &[a]).unwrap(), 0);

        assert_eq!(collections_holding(&db, &[a, b]).unwrap(), [both]);
        let mut holding_a = collections_holding(&db, &[a]).unwrap();
        holding_a.sort();
        assert_eq!(holding_a, [both, only_a]);
    }

    #[test]
    fn a_forgotten_sample_leaves_its_collections() {
        let db = new_db();
        let id = add(&db, "/lib/a.wav");
        let collection = create_collection(&db, "Set").unwrap();
        add_to_collection(&db, collection, &[id]).unwrap();
        forget(&db, Path::new("/lib/a.wav")).unwrap();
        assert_eq!(collections(&db).unwrap()[0].samples, 0);
    }

    #[test]
    fn deleting_a_colour_leaves_its_tags_without_colour() {
        let db = new_db();
        let id = add(&db, "/lib/a.wav");
        tag(&db, id, &["drums"]);
        let color = add_color(&db, "Lime", "#a0e040").unwrap();
        set_tag_color(&db, "drums", Some(color)).unwrap();
        assert_eq!(tag_colors(&db).unwrap(), [("drums".to_string(), color)]);
        delete_color(&db, color).unwrap();
        assert!(tag_colors(&db).unwrap().is_empty());
        assert_eq!(tags_of(&db, id).unwrap(), ["drums"]);
    }

    #[test]
    fn a_waveform_is_stale_once_the_file_changes() {
        let db = new_db();
        let id = add(&db, "/lib/a.wav");
        let stamp = FileStamp {
            size: 100,
            modified: 1_000,
        };
        let peaks = vec![(-0.5, 0.5); 4];
        save_waveform(&db, id, stamp, &peaks, waveform::Measures::default()).unwrap();
        assert!(has_waveform(&db, id, stamp).unwrap());
        assert!(waveform(&db, id, stamp).unwrap().is_some());
        let changed = FileStamp {
            size: 100,
            modified: 2_000,
        };
        assert!(!has_waveform(&db, id, changed).unwrap());
        assert!(waveform(&db, id, changed).unwrap().is_none());
    }

    /// The only sample found by `words`, as the interface gets it.
    fn shown(db: &Connection, path: &str) -> SampleInfo {
        let mut samples = search(db, &[], &[path], false, None).unwrap();
        assert_eq!(samples.len(), 1);
        samples.remove(0)
    }

    /// Tempo 120 and key Am from its name, a loop from its folder.
    const BASS: &str = "/lib/Loops/Bass 120BPM Am.wav";

    #[test]
    fn a_tempo_set_by_hand() {
        let db = new_db();
        let id = add(&db, BASS);
        let tempo = |db: &Connection| {
            let sample = shown(db, BASS);
            (sample.bpm, sample.bpm_source, sample.bpm_fixed)
        };
        assert_eq!(tempo(&db), (Some(120), Some(analysis::Source::Name), false));
        set_tempo(&db, id, Some(90)).unwrap();
        assert_eq!(
            tempo(&db),
            (Some(90), Some(analysis::Source::Manual), false)
        );
        // 0: none, to silence a wrong guess.
        set_tempo(&db, id, Some(0)).unwrap();
        assert_eq!(tempo(&db), (None, None, false));
        set_tempo(&db, id, None).unwrap();
        assert_eq!(tempo(&db), (Some(120), Some(analysis::Source::Name), false));
    }

    #[test]
    fn a_tempo_chosen_with_detect_bpm_stays_until_typed_over() {
        let db = new_db();
        let id = add(&db, BASS);
        set_detected_tempo(&db, id, 100, "sound").unwrap();
        let sample = shown(&db, BASS);
        assert_eq!(sample.bpm, Some(100));
        assert_eq!(sample.bpm_source, Some(analysis::Source::Sound));
        assert!(sample.bpm_fixed);
        // Typed by hand afterwards: no longer "chosen with Detect BPM".
        set_tempo(&db, id, Some(95)).unwrap();
        let sample = shown(&db, BASS);
        assert_eq!(sample.bpm_source, Some(analysis::Source::Manual));
        assert!(!sample.bpm_fixed);
    }

    #[test]
    fn the_tempo_detected_from_the_sound_is_a_loop_s_last_resort() {
        let db = new_db();
        let measures = waveform::Measures {
            tempo: Some(97),
            ..Default::default()
        };
        let stamp = FileStamp {
            size: 1,
            modified: 1,
        };
        // A loop without a tempo in its name: the detected one replaces the
        // estimate from the length.
        let groove = add(&db, "/lib/Loops/Groove.wav");
        // Two seconds: four beats at 120, the estimate from the length.
        db.execute(
            "UPDATE files SET duration_secs = 2.0 WHERE id = ?1",
            [groove],
        )
        .unwrap();
        let before = shown(&db, "Groove");
        assert_eq!(before.bpm, Some(120));
        assert_eq!(before.bpm_source, Some(analysis::Source::Length));
        save_waveform(&db, groove, stamp, &[], measures).unwrap();
        let sample = shown(&db, "Groove");
        assert_eq!(sample.bpm, Some(97));
        assert_eq!(sample.bpm_source, Some(analysis::Source::Sound));
        // The name comes first.
        let bass = add(&db, BASS);
        save_waveform(&db, bass, stamp, &[], measures).unwrap();
        assert_eq!(shown(&db, BASS).bpm, Some(120));
        // A tempo silenced by hand stays silent.
        set_tempo(&db, groove, Some(0)).unwrap();
        assert_eq!(shown(&db, "Groove").bpm, None);
        // A one-shot gets none, and does not become a loop.
        let kick = add(&db, "/lib/One Shots/Kick.wav");
        save_waveform(&db, kick, stamp, &[], measures).unwrap();
        let sample = shown(&db, "Kick");
        assert_ne!(sample.bpm, Some(97));
        assert!(!sample.looped);
    }

    #[test]
    fn a_key_set_by_hand() {
        let db = new_db();
        let id = add(&db, BASS);
        assert_eq!(shown(&db, BASS).key.as_deref(), Some("Am"));
        set_key(&db, id, Some("C")).unwrap();
        assert_eq!(shown(&db, BASS).key.as_deref(), Some("C"));
        set_key(&db, id, Some("")).unwrap();
        assert_eq!(shown(&db, BASS).key, None);
        set_key(&db, id, None).unwrap();
        assert_eq!(shown(&db, BASS).key.as_deref(), Some("Am"));
    }

    #[test]
    fn a_loop_set_by_hand() {
        let db = new_db();
        let id = add(&db, BASS);
        let looped = |db: &Connection| {
            let sample = shown(db, BASS);
            (sample.looped, sample.loop_source)
        };
        assert_eq!(looped(&db), (true, Some(analysis::Source::Name)));
        set_loop(&db, id, Some(false)).unwrap();
        assert_eq!(looped(&db), (false, Some(analysis::Source::Manual)));
        set_loop(&db, id, None).unwrap();
        assert_eq!(looped(&db), (true, Some(analysis::Source::Name)));
    }

    #[test]
    fn a_type_set_by_hand() {
        let db = new_db();
        let id = add(&db, BASS);
        let kind = |db: &Connection| {
            let sample = shown(db, BASS);
            (sample.sound_type, sample.type_source)
        };
        assert_eq!(
            kind(&db),
            (Some("bass".to_string()), Some(analysis::Source::Name))
        );
        set_sound_type(&db, id, Some("keys/piano")).unwrap();
        assert_eq!(
            kind(&db),
            (
                Some("keys/piano".to_string()),
                Some(analysis::Source::Manual)
            )
        );
        // '': no type, set by hand.
        set_sound_type(&db, id, Some("")).unwrap();
        assert_eq!(kind(&db), (None, Some(analysis::Source::Manual)));
        // A type no longer in the list counts as not set.
        set_sound_type(&db, id, Some("drums/theremin")).unwrap();
        assert_eq!(
            kind(&db),
            (Some("bass".to_string()), Some(analysis::Source::Name))
        );
        set_sound_type(&db, id, None).unwrap();
        assert_eq!(
            kind(&db),
            (Some("bass".to_string()), Some(analysis::Source::Name))
        );
    }
}
