// samplr's SQLite database: the known samples and their tags.
// Tags live here, never inside the audio files.

use std::env;
use std::error::Error;
use std::fs;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, named_params, params, params_from_iter};
use serde::Serialize;

use crate::waveform::{self, Peak};
use crate::{SampleInfo, analysis};

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
    migrate(&db)?;

    Ok(db)
}

/// Brings a database created by an older samplr up to date. Its version is
/// kept by SQLite (`user_version`, 0 for a new database); each step runs once.
/// A new database goes through every step too: the tables above are those of
/// version 0.
fn migrate(db: &Connection) -> Result<(), Box<dyn Error>> {
    // `IMMEDIATE` takes the write lock before reading the version: the app
    // and a `samplr` command opening the database together cannot both run
    // the same step.
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
    let rows = query.query_map([], |row| row.get::<_, String>(0))?;

    let mut folders = Vec::new();
    for path in rows {
        folders.push(PathBuf::from(path?));
    }
    Ok(folders)
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
        |row| row.get::<_, String>(0),
    )?;

    let mut files = Vec::new();
    for path in rows {
        files.push(PathBuf::from(path?));
    }
    Ok(files)
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
    // If a sample already sat at the new path (overwritten file), it is
    // replaced: otherwise the UNIQUE constraint would make the update fail.
    forget(db, new)?;
    // Replaces the start of the path: `/a/kick.wav` becomes `/b/kick.wav`,
    // `/a/drums/kick.wav` becomes `/b/drums/kick.wav`.
    let old = path_as_text(old)?;
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
        "INSERT INTO files (path, duration_secs, sample_rate, channels, bits)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (path) DO UPDATE SET
             duration_secs = excluded.duration_secs,
             sample_rate   = excluded.sample_rate,
             channels      = excluded.channels,
             bits          = excluded.bits,
             hidden        = 0
         RETURNING id",
        params![
            path,
            info.duration_secs,
            info.sample_rate,
            info.channels,
            info.bits
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

    let mut files = Vec::new();
    for file in rows {
        files.push(file?);
    }
    Ok(files)
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
            "SELECT peaks FROM waveforms WHERE file_id = ?1 AND size = ?2 AND modified = ?3",
            params![file_id, stamp.size, stamp.modified],
            |row| row.get(0),
        )
        .optional()?;
    Ok(bytes.map(|b| waveform::from_bytes(&b)))
}

/// Saves a sample's waveform, replacing the previous one.
pub fn save_waveform(
    db: &Connection,
    file_id: i64,
    stamp: FileStamp,
    peaks: &[Peak],
) -> Result<(), Box<dyn Error>> {
    db.execute(
        "INSERT OR REPLACE INTO waveforms (file_id, size, modified, peaks)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            file_id,
            stamp.size,
            stamp.modified,
            waveform::to_bytes(peaks)
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

    let mut colors = Vec::new();
    for color in rows {
        colors.push(color?);
    }
    Ok(colors)
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

    let mut tags = Vec::new();
    for tag in rows {
        tags.push(tag?);
    }
    Ok(tags)
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

    let mut collections = Vec::new();
    for collection in rows {
        collections.push(collection?);
    }
    Ok(collections)
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
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; file_ids.len()].join(", ");
    let mut query = db.prepare(&format!(
        "SELECT collection_id FROM collection_files
         WHERE file_id IN ({marks})
         GROUP BY collection_id
         HAVING COUNT(*) = {}",
        file_ids.len()
    ))?;
    let rows = query.query_map(params_from_iter(file_ids), |row| row.get(0))?;

    let mut ids = Vec::new();
    for id in rows {
        ids.push(id?);
    }
    Ok(ids)
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

    let mut tags = Vec::new();
    for tag in rows {
        tags.push(tag?);
    }
    Ok(tags)
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

    let mut tags = Vec::new();
    for tag in rows {
        tags.push(tag?);
    }
    Ok(tags)
}

/// The samples carrying **all** the given `tags`, whose path contains `text`,
/// and where each of the `words` is found in the path or in one of the tags
/// (case-insensitive), sorted by path. With no tag, no word and an empty
/// text, every sample shown. Hidden samples (see `remove_folder`) are left
/// out. `favorites_only`: only the favorites. `collection`: only the samples
/// of that collection.
pub fn search(
    db: &Connection,
    tags: &[String],
    words: &[&str],
    text: &str,
    favorites_only: bool,
    collection: Option<i64>,
) -> Result<Vec<SampleInfo>, Box<dyn Error>> {
    // `instr` looks for the text as is: with `LIKE`, a `_` in the text
    // (common in sample names) would act as a wildcard.
    let mut sql = String::from(
        "SELECT f.path, f.duration_secs, f.sample_rate, f.channels, f.bits,
                (SELECT group_concat(t.name, ' ' ORDER BY t.name)
                 FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
                 WHERE ft.file_id = f.id),
                f.favorite,
                (SELECT group_concat(cf.collection_id) FROM collection_files cf
                 WHERE cf.file_id = f.id)
         FROM files f
         WHERE f.hidden = 0 AND instr(lower(f.path), lower(?1)) > 0",
    );
    if favorites_only {
        sql.push_str(" AND f.favorite = 1");
    }
    let mut values = vec![text.to_string()];
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
    // Tags are stored lowercase: only the word needs `lower`.
    for word in words {
        values.push(word.to_string());
        let n = values.len();
        sql.push_str(&format!(
            " AND (instr(lower(f.path), lower(?{n})) > 0
                   OR EXISTS (SELECT 1 FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
                              WHERE ft.file_id = f.id AND instr(t.name, lower(?{n})) > 0))"
        ));
    }
    sql.push_str(" ORDER BY f.path");

    let mut query = db.prepare(&sql)?;
    let rows = query.query_map(params_from_iter(&values), |row| {
        let path = PathBuf::from(row.get::<_, String>(0)?);
        let tags: Option<String> = row.get(5)?;
        let duration_secs: Option<f64> = row.get(1)?;
        // Worked out from the name and the length at each search, rather
        // than stored: a renamed file needs nothing more.
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let tempo = analysis::tempo(stem, duration_secs);
        let key = analysis::key_from_name(stem);
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
            key,
            path,
        })
    })?;

    let mut samples = Vec::new();
    for sample in rows {
        samples.push(sample?);
    }
    Ok(samples)
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
