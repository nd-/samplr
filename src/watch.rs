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

use crate::{db, index, is_audio, is_hidden_below, read_info};

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
    // Unplugged disk: every sample would look vanished, and forgetting them
    // would lose their tags.
    if !root.is_dir() {
        report(format!("{}: not found, nothing changed", root.display()));
        return Ok(());
    }
    // Which files vanished is found outside any transaction (one disk access
    // per sample); forgetting them is then quick. The folder is read again by
    // `index`, which saves in short transactions of its own.
    // A file in a hidden folder counts as vanished: an older samplr may
    // have imported a disk's trash (see `is_hidden_below`).
    let roots = [root.to_path_buf()];
    let vanished: Vec<PathBuf> = db::files_under(db, root)?
        .into_iter()
        .filter(|path| !path.exists() || is_hidden_below(path, &roots))
        .collect();
    let transaction = db.transaction()?;
    let mut removed = 0;
    for path in &vanished {
        removed += db::forget(&transaction, path)?;
    }
    transaction.commit()?;
    let (samples, unreadable) = index(db, root, &mut |_| {});

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
            // Moved into a hidden folder (a disk's trash): gone from the
            // library, as if deleted.
            if is_hidden_below(new, roots) {
                return sync(db, roots, old, false, report);
            }
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
    // Hidden below a scanned folder (see `is_hidden_below`): never a sample.
    // Anything recorded there by an older samplr is forgotten.
    if is_hidden_below(path, roots) {
        let forgotten = db::forget(db, path)?;
        if forgotten > 0 {
            report(format!("- {} ({forgotten} sample(s))", path.display()));
        }
        return Ok(());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, memory_db, write_wav};
    use notify_debouncer_full::notify::Event;
    use notify_debouncer_full::notify::event::{CreateKind, RemoveKind};
    use std::fs;
    use std::time::Instant;

    fn event(kind: EventKind, paths: &[&Path]) -> DebouncedEvent {
        let mut event = Event::new(kind);
        for path in paths {
            event = event.add_path(path.to_path_buf());
        }
        DebouncedEvent::new(event, Instant::now())
    }

    fn known(db: &Connection, root: &Path) -> Vec<PathBuf> {
        let mut paths = db::files_under(db, root).unwrap();
        paths.sort();
        paths
    }

    fn ignore(_: String) {}

    #[test]
    fn a_folder_inside_another_is_watched_once() {
        let folders = [
            PathBuf::from("/lib"),
            PathBuf::from("/lib/drums"),
            PathBuf::from("/other"),
        ];
        assert_eq!(
            roots(&folders),
            [PathBuf::from("/lib"), PathBuf::from("/other")]
        );
    }

    #[test]
    fn catching_up_forgets_deleted_files_and_adds_new_ones() {
        let dir = TempDir::new();
        let root = dir.path();
        let (gone, kept, new) = (root.join("a.wav"), root.join("b.wav"), root.join("c.wav"));
        write_wav(&gone);
        write_wav(&kept);
        let mut db = memory_db();
        index(&db, root, &mut |_| {});
        let kept_id = db::file_id(&db, &kept).unwrap().unwrap();
        db::add_tags(&db, kept_id, &["drums".to_string()]).unwrap();

        // While samplr was closed.
        fs::remove_file(&gone).unwrap();
        write_wav(&new);
        catch_up(&mut db, root, &mut ignore).unwrap();

        assert_eq!(known(&db, root), [kept.clone(), new]);
        assert_eq!(db::file_id(&db, &kept).unwrap(), Some(kept_id));
        assert_eq!(db::tags_of(&db, kept_id).unwrap(), ["drums"]);
    }

    #[test]
    fn catching_up_forgets_what_lies_in_a_hidden_folder() {
        let dir = TempDir::new();
        let root = dir.path();
        let trashed = root.join(".Trash-1000/files/kick.wav");
        write_wav(&trashed);
        let mut db = memory_db();
        // Imported by an older samplr, which did not skip hidden folders.
        db::save(&db, &crate::read_info(&trashed).unwrap()).unwrap();

        catch_up(&mut db, root, &mut ignore).unwrap();
        assert!(known(&db, root).is_empty());
    }

    #[test]
    fn catching_up_with_an_unplugged_disk_forgets_nothing() {
        let dir = TempDir::new();
        let root = dir.path().join("disk");
        write_wav(&root.join("kick.wav"));
        let mut db = memory_db();
        index(&db, &root, &mut |_| {});
        fs::remove_dir_all(&root).unwrap();

        catch_up(&mut db, &root, &mut ignore).unwrap();
        assert_eq!(known(&db, &root).len(), 1);
    }

    #[test]
    fn a_file_removed_live_is_forgotten() {
        let dir = TempDir::new();
        let root = dir.path();
        let kick = root.join("kick.wav");
        write_wav(&kick);
        let db = memory_db();
        index(&db, root, &mut |_| {});
        fs::remove_file(&kick).unwrap();

        let removed = event(EventKind::Remove(RemoveKind::File), &[&kick]);
        handle_event(&db, &[root.to_path_buf()], &removed, &mut ignore).unwrap();
        assert!(known(&db, root).is_empty());
    }

    #[test]
    fn a_disk_unplugged_live_forgets_nothing() {
        let dir = TempDir::new();
        let root = dir.path().join("disk");
        let kick = root.join("kick.wav");
        write_wav(&kick);
        let db = memory_db();
        index(&db, &root, &mut |_| {});
        fs::remove_dir_all(&root).unwrap();

        let removed = event(EventKind::Remove(RemoveKind::Any), &[&kick]);
        handle_event(&db, std::slice::from_ref(&root), &removed, &mut ignore).unwrap();
        assert_eq!(known(&db, &root), [kick]);
    }

    #[test]
    fn a_file_created_live_is_added() {
        let dir = TempDir::new();
        let root = dir.path();
        let db = memory_db();
        let kick = root.join("drums/kick.wav");
        write_wav(&kick);

        let created = event(
            EventKind::Create(CreateKind::Folder),
            &[&root.join("drums")],
        );
        handle_event(&db, &[root.to_path_buf()], &created, &mut ignore).unwrap();
        assert_eq!(known(&db, root), [kick]);
    }

    #[test]
    fn a_rename_keeps_the_tags() {
        let dir = TempDir::new();
        let root = dir.path();
        let (old, new) = (root.join("kick.wav"), root.join("deep/kick 2.wav"));
        write_wav(&old);
        let db = memory_db();
        index(&db, root, &mut |_| {});
        let id = db::file_id(&db, &old).unwrap().unwrap();
        db::add_tags(&db, id, &["drums".to_string()]).unwrap();
        fs::create_dir_all(new.parent().unwrap()).unwrap();
        fs::rename(&old, &new).unwrap();

        let renamed = event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &[&old, &new],
        );
        handle_event(&db, &[root.to_path_buf()], &renamed, &mut ignore).unwrap();
        assert_eq!(db::file_id(&db, &new).unwrap(), Some(id));
        assert_eq!(db::tags_of(&db, id).unwrap(), ["drums"]);
    }

    #[test]
    fn a_file_moved_to_the_trash_is_forgotten() {
        let dir = TempDir::new();
        let root = dir.path();
        let (old, new) = (root.join("kick.wav"), root.join(".Trashes/kick.wav"));
        write_wav(&old);
        let db = memory_db();
        index(&db, root, &mut |_| {});
        fs::create_dir_all(new.parent().unwrap()).unwrap();
        fs::rename(&old, &new).unwrap();

        let moved = event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &[&old, &new],
        );
        handle_event(&db, &[root.to_path_buf()], &moved, &mut ignore).unwrap();
        assert!(known(&db, root).is_empty());
    }

    #[test]
    fn a_sample_renamed_to_another_extension_is_forgotten() {
        let dir = TempDir::new();
        let root = dir.path();
        let (old, new) = (root.join("kick.wav"), root.join("kick.bak"));
        write_wav(&old);
        let db = memory_db();
        index(&db, root, &mut |_| {});
        fs::rename(&old, &new).unwrap();

        let renamed = event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &[&old, &new],
        );
        handle_event(&db, &[root.to_path_buf()], &renamed, &mut ignore).unwrap();
        assert!(known(&db, root).is_empty());
    }
}
