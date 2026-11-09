// Following the scanned folders live, shared by `samplr watch` and the
// graphical interface. What changed is told through `report`: the command
// line prints it, the interface refreshes its list.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use notify_debouncer_full::notify::event::{ModifyKind, RenameMode};
use notify_debouncer_full::notify::{EventKind, RecommendedWatcher};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache, new_debouncer,
};
use rusqlite::Connection;

use crate::{db, index, is_audio, read_info};

pub use notify_debouncer_full::notify::RecursiveMode;

/// Watches folders; its events arrive in the `Receiver` given by `new_watcher`.
pub type Watcher = Debouncer<RecommendedWatcher, RecommendedCache>;

/// Starts a watcher, watching nothing yet. Events for the same file are
/// grouped for one second: a copy produces dozens of writes, the file is read
/// again only once it is done.
pub fn new_watcher() -> Result<(Watcher, Receiver<DebounceEventResult>), Box<dyn Error>> {
    // The watcher runs in its own thread and drops its events into a
    // channel: `sender` stays on its side, `receiver` on ours.
    let (sender, receiver) = mpsc::channel();
    let watcher = new_debouncer(Duration::from_secs(1), None, sender)?;
    Ok((watcher, receiver))
}

/// The folders to watch among `folders`: a folder inside another one is
/// already followed with it, and watching it twice would double every event.
pub fn roots(folders: &[PathBuf]) -> Vec<PathBuf> {
    folders
        .iter()
        .filter(|f| {
            !folders
                .iter()
                .any(|other| other != *f && f.starts_with(other))
        })
        .cloned()
        .collect()
}

/// Updates the database for `root`: removes the samples gone from the disk,
/// adds the new ones and refreshes the others.
pub fn catch_up(
    db: &mut Connection,
    root: &Path,
    report: &mut dyn FnMut(String),
) -> Result<(), Box<dyn Error>> {
    let transaction = db.transaction()?;

    let mut removed = 0;
    for path in db::files_under(&transaction, root)? {
        if !path.exists() {
            removed += db::forget(&transaction, &path)?;
        }
    }
    let (samples, unreadable) = index(&transaction, root, &mut |_| {});
    transaction.commit()?;

    report(format!(
        "{}: {} sample(s), {removed} removed, {unreadable} unreadable",
        root.display(),
        samples.len()
    ));
    Ok(())
}

/// Applies a file system event to the database. `roots`: the watched folders.
pub fn handle_event(
    db: &Connection,
    roots: &[PathBuf],
    event: &DebouncedEvent,
    report: &mut dyn FnMut(String),
) -> Result<(), Box<dyn Error>> {
    match event.kind {
        // A rename whose both ends are known: the existing row is moved to
        // keep the tags.
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            let (old, new) = (&event.paths[0], &event.paths[1]);
            let renamed = db::rename(db, old, new)?;
            if renamed > 0 {
                report(format!("→ {} → {}", old.display(), new.display()));
            }
            // The new name may change things: `kick.tmp` turned into
            // `kick.wav` or a folder coming from elsewhere (nothing to rename
            // in the database), `kick.wav` turned into `kick.bak` (no longer
            // a sample).
            if renamed == 0 || (new.is_file() && !is_audio(new)) {
                sync(db, roots, new, true, report)?;
            }
            Ok(())
        }
        // Reading a file changes nothing for the database.
        EventKind::Access(_) => Ok(()),
        _ => {
            let appeared = matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
            );
            for path in &event.paths {
                sync(db, roots, path, appeared, report)?;
            }
            Ok(())
        }
    }
}

/// Aligns the database with what currently sits at `path`.
/// `appeared`: the path has just appeared (creation or rename). A folder is
/// only walked in that case; otherwise the slightest change inside a folder
/// would read its whole content again.
fn sync(
    db: &Connection,
    roots: &[PathBuf],
    path: &Path,
    appeared: bool,
    report: &mut dyn FnMut(String),
) -> Result<(), Box<dyn Error>> {
    if !path.exists() {
        // Unplugged disk: everything disappears at once, but the samples
        // still exist. Forgetting them would lose their tags.
        let root_present = roots
            .iter()
            .any(|root| path.starts_with(root) && root.is_dir());
        if !root_present {
            return Ok(());
        }
        let forgotten = db::forget(db, path)?;
        if forgotten > 0 {
            report(format!("- {} ({forgotten} sample(s))", path.display()));
        }
    } else if path.is_dir() {
        if appeared {
            let (samples, _) = index(db, path, &mut |_| {});
            if !samples.is_empty() {
                report(format!(
                    "+ {} ({} sample(s))",
                    path.display(),
                    samples.len()
                ));
            }
        }
    } else if is_audio(path) {
        // A file being copied may still be unreadable: the end of the copy
        // will produce a new event, and it will be tried again.
        if let Ok(info) = read_info(path) {
            db::save(db, &info)?;
            report(format!("+ {}", path.display()));
        }
    } else if db::forget(db, path)? > 0 {
        // Renamed to a name that is no longer audio (`kick.wav` → `kick.bak`).
        report(format!("- {}", path.display()));
    }
    Ok(())
}
