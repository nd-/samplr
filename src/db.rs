// samplr's SQLite database: the known samples and their tags.
// Tags live here, never inside the audio files.

use std::env;
use std::error::Error;
use std::fs;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, named_params, params, params_from_iter};

use crate::SampleInfo;

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
        );",
    )?;

    Ok(db)
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
    // that `id` in both cases, insert or update.
    let id = db.query_row(
        "INSERT INTO files (path, duration_secs, sample_rate, channels, bits)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (path) DO UPDATE SET
             duration_secs = excluded.duration_secs,
             sample_rate   = excluded.sample_rate,
             channels      = excluded.channels,
             bits          = excluded.bits
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

/// Every tag, with the number of samples carrying it, in alphabetical order.
pub fn list_tags(db: &Connection) -> Result<Vec<(String, u32)>, Box<dyn Error>> {
    let mut query = db.prepare(
        "SELECT t.name, COUNT(*)
         FROM tags t JOIN file_tags ft ON ft.tag_id = t.id
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

/// The samples carrying **all** the given `tags` and whose path contains
/// `text` (case-insensitive), sorted by path. With no tag and an empty text,
/// every sample in the database.
pub fn search(
    db: &Connection,
    tags: &[String],
    text: &str,
) -> Result<Vec<SampleInfo>, Box<dyn Error>> {
    // `instr` looks for the text as is: with `LIKE`, a `_` in the text
    // (common in sample names) would act as a wildcard.
    let mut sql = String::from(
        "SELECT f.path, f.duration_secs, f.sample_rate, f.channels, f.bits,
                (SELECT group_concat(t.name, ' ' ORDER BY t.name)
                 FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
                 WHERE ft.file_id = f.id)
         FROM files f
         WHERE instr(lower(f.path), lower(?1)) > 0",
    );
    let mut values = vec![text.to_string()];

    // The number of tags varies: one condition, and one `?N`, per tag.
    for tag in tags {
        values.push(normalize_tag(tag)?);
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
                          WHERE ft.file_id = f.id AND t.name = ?{})",
            values.len()
        ));
    }
    sql.push_str(" ORDER BY f.path");

    let mut query = db.prepare(&sql)?;
    let rows = query.query_map(params_from_iter(&values), |row| {
        let path: String = row.get(0)?;
        let tags: Option<String> = row.get(5)?;
        Ok(SampleInfo {
            path: PathBuf::from(path),
            duration_secs: row.get(1)?,
            sample_rate: row.get(2)?,
            channels: row.get(3)?,
            bits: row.get(4)?,
            tags: tags
                .map(|t| t.split(' ').map(String::from).collect())
                .unwrap_or_default(),
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
