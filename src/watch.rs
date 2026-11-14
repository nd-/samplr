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

use crate::{db, index, is_audio, is_hidden_below, read_info, scanned_folder_of};

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
/// adds the new ones and refreshes the others. The other scanned folders are
/// read from the database: one inside `root` may be a disk of its own, or a
/// hidden folder the walk of `root` skips.
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
    let mut folders = db::folders(db)?;
    if !folders.iter().any(|folder| folder == root) {
        folders.push(root.to_path_buf());
    }
    // Which files vanished is found outside any transaction (one disk access
    // per sample); forgetting them is then quick. The folder is read again by
    // `index`, which saves in short transactions of its own.
    let vanished: Vec<PathBuf> = db::files_under(db, root)?
        .into_iter()
        .filter(|path| no_longer_a_sample(path, &folders))
        .collect();
    let transaction = db.transaction()?;
    let mut removed = 0;
    for path in &vanished {
        removed += db::forget(&transaction, path)?;
    }
    transaction.commit()?;
    let (samples, mut unreadable) = index(db, root, &mut |_| {});
    let mut found = samples.len();
    let walked = [root.to_path_buf()];
    for folder in &folders {
        if folder != root && is_hidden_below(folder, &walked) && folder.is_dir() {
            let (samples, more_unreadable) = index(db, folder, &mut |_| {});
            found += samples.len();
            unreadable += more_unreadable;
        }
    }

    report(format!(
        "{}: {found} sample(s), {removed} removed, {unreadable} unreadable",
        root.display(),
    ));
    Ok(())
}

/// Whether the sample recorded at `path` is to be forgotten: hidden below
/// its scanned folder (an older samplr may have imported a disk's trash, see
/// `is_hidden_below`), turned into a folder, or proven gone. A file that
/// cannot be reached (a folder without permission) or whose scanned folder
/// is missing (a disk unplugged) is kept with its tags.
fn no_longer_a_sample(path: &Path, folders: &[PathBuf]) -> bool {
    if is_hidden_below(path, folders) {
        return true;
    }
    match path.try_exists() {
        Ok(true) => path.is_dir(),
        Ok(false) => scanned_folder_of(path, folders).is_some_and(|folder| folder.is_dir()),
        Err(_) => false,
    }
}

/// Applies a file system event to the database. `folders`: every scanned
/// folder (`db::folders`), not only the watched ones: a scanned folder inside
/// another one has its own hidden files and its own disk.
pub fn handle_event(
    db: &Connection,
    folders: &[PathBuf],
    event: &DebouncedEvent,
    report: &mut dyn FnMut(String),
) -> Result<(), Box<dyn Error>> {
    match event.kind {
        // A rename whose both ends are known: the existing row is moved to
        // keep the tags.
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            let (old, new) = (&event.paths[0], &event.paths[1]);
            // Moved into a hidden folder (a disk's trash) or out of the
            // scanned folders: gone from the library, as if deleted.
            if is_hidden_below(new, folders) || scanned_folder_of(new, folders).is_none() {
                return sync(db, folders, old, false, report);
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
                sync(db, folders, new, true, report)?;
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
                sync(db, folders, path, appeared, report)?;
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
    folders: &[PathBuf],
    path: &Path,
    appeared: bool,
    report: &mut dyn FnMut(String),
) -> Result<(), Box<dyn Error>> {
    // Outside the scanned folders: an event queued before its folder was
    // removed would bring the folder's hidden samples back.
    let Some(folder) = scanned_folder_of(path, folders) else {
        return Ok(());
    };
    // Hidden below a scanned folder (see `is_hidden_below`): never a sample.
    // Anything recorded there by an older samplr is forgotten.
    if is_hidden_below(path, folders) {
        let forgotten = db::forget(db, path)?;
        if forgotten > 0 {
            report(format!("- {} ({forgotten} sample(s))", path.display()));
        }
        return Ok(());
    }
    // Only a proven absence forgets: a path that cannot be reached (no
    // permission) still holds its samples.
    let Ok(exists) = path.try_exists() else {
        return Ok(());
    };
    if !exists {
        // Unplugged disk: everything disappears at once, but the samples
        // still exist. Forgetting them would lose their tags.
        if !folder.is_dir() {
            return Ok(());
        }
        let forgotten = db::forget(db, path)?;
        if forgotten > 0 {
            report(format!("- {} ({forgotten} sample(s))", path.display()));
        }
    } else if path.is_dir() {
        // A file replaced by a folder of the same name.
        if db::forget_file(db, path)? > 0 {
            report(format!("- {}", path.display()));
        }
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

    fn rename_event(old: &Path, new: &Path) -> DebouncedEvent {
        event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &[old, new],
        )
    }

    fn apply(db: &Connection, roots: &[PathBuf], event: &DebouncedEvent) {
        handle_event(db, roots, event, &mut ignore).unwrap();
    }

    /// Indexes `root` and tags `path` with `tag`; returns its `id`.
    fn tagged(db: &Connection, root: &Path, path: &Path, tag: &str) -> i64 {
        index(db, root, &mut |_| {});
        let id = db::file_id(db, path).unwrap().unwrap();
        db::add_tags(db, id, &[tag.to_string()]).unwrap();
        id
    }

    fn is_hidden(db: &Connection, path: &Path) -> bool {
        db.query_row(
            "SELECT hidden FROM files WHERE path = ?1",
            [path.to_str().unwrap()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            == 1
    }

    /// Takes the read and execute permissions from a folder, and gives them
    /// back when dropped, so that `TempDir` can delete it.
    struct Locked(PathBuf);

    impl Locked {
        /// `None` when the lock does not hold (running as root).
        #[cfg(unix)]
        fn new(folder: &Path) -> Option<Locked> {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(folder, fs::Permissions::from_mode(0o000)).unwrap();
            let locked = Locked(folder.to_path_buf());
            if fs::read_dir(folder).is_ok() {
                eprintln!("running as root: permissions not tested");
                return None;
            }
            Some(locked)
        }
    }

    #[cfg(unix)]
    impl Drop for Locked {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }

    #[test]
    fn roots_compare_whole_folder_names() {
        let folders = [
            PathBuf::from("/lib"),
            PathBuf::from("/library"),
            PathBuf::from("/lib/drums/deep"),
            PathBuf::from("/lib/drums"),
        ];
        assert_eq!(
            roots(&folders),
            [PathBuf::from("/lib"), PathBuf::from("/library")]
        );
        assert!(roots(&[]).is_empty());
    }

    #[test]
    fn an_unplugged_disk_plugged_back_keeps_its_tags() {
        let dir = TempDir::new();
        let root = dir.path().join("disk");
        let kick = root.join("drums/kick.wav");
        write_wav(&kick);
        let mut db = memory_db();
        let id = tagged(&db, &root, &kick, "drums");
        let roots = [root.clone()];

        let backup = dir.path().join("elsewhere");
        fs::rename(&root, &backup).unwrap();
        // Live, the disk's going is reported for the root, a folder and a file.
        for path in [&root, &root.join("drums"), &kick] {
            apply(
                &db,
                &roots,
                &event(EventKind::Remove(RemoveKind::Any), &[path]),
            );
        }
        apply(
            &db,
            &roots,
            &event(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                &[&root],
            ),
        );
        catch_up(&mut db, &root, &mut ignore).unwrap();
        assert_eq!(known(&db, &root), std::slice::from_ref(&kick));

        fs::rename(&backup, &root).unwrap();
        catch_up(&mut db, &root, &mut ignore).unwrap();
        assert_eq!(db::file_id(&db, &kick).unwrap(), Some(id));
        assert_eq!(db::tags_of(&db, id).unwrap(), ["drums"]);
    }

    #[test]
    fn a_subfolder_removed_while_its_root_remains_is_forgotten() {
        let dir = TempDir::new();
        let root = dir.path();
        let (kick, snare) = (root.join("drums/kick.wav"), root.join("snare.wav"));
        write_wav(&kick);
        write_wav(&snare);
        let db = memory_db();
        index(&db, root, &mut |_| {});
        fs::remove_dir_all(root.join("drums")).unwrap();

        // The debouncer reports a folder's removal once, not its files'.
        apply(
            &db,
            &[root.to_path_buf()],
            &event(
                EventKind::Remove(RemoveKind::Folder),
                &[&root.join("drums")],
            ),
        );
        assert_eq!(known(&db, root), [snare]);
    }

    #[test]
    fn a_subfolder_removed_while_closed_is_forgotten_at_catch_up() {
        let dir = TempDir::new();
        let root = dir.path();
        let (kick, snare) = (root.join("drums/kick.wav"), root.join("snare.wav"));
        write_wav(&kick);
        write_wav(&snare);
        let mut db = memory_db();
        index(&db, root, &mut |_| {});
        fs::remove_dir_all(root.join("drums")).unwrap();

        catch_up(&mut db, root, &mut ignore).unwrap();
        assert_eq!(known(&db, root), [snare]);
    }

    #[test]
    fn a_folder_renamed_keeps_the_ids_and_tags_of_its_samples() {
        let dir = TempDir::new();
        let root = dir.path();
        let (old_dir, new_dir) = (root.join("drums"), root.join("Drum Kit #2"));
        let (kick, snare) = (old_dir.join("kick.wav"), old_dir.join("sub/snare.wav"));
        write_wav(&kick);
        write_wav(&snare);
        let db = memory_db();
        let kick_id = tagged(&db, root, &kick, "kick");
        let snare_id = tagged(&db, root, &snare, "snare");
        fs::rename(&old_dir, &new_dir).unwrap();

        apply(
            &db,
            &[root.to_path_buf()],
            &rename_event(&old_dir, &new_dir),
        );
        let (new_kick, new_snare) = (new_dir.join("kick.wav"), new_dir.join("sub/snare.wav"));
        assert_eq!(known(&db, root), [new_kick.clone(), new_snare.clone()]);
        assert_eq!(db::file_id(&db, &new_kick).unwrap(), Some(kick_id));
        assert_eq!(db::file_id(&db, &new_snare).unwrap(), Some(snare_id));
        assert_eq!(db::tags_of(&db, snare_id).unwrap(), ["snare"]);
    }

    #[test]
    fn renames_to_and_from_unusual_names_keep_the_id() {
        let dir = TempDir::new();
        let root = dir.path();
        let names = [
            "kick.wav",
            // "été" composed (NFC), then decomposed (NFD): two names on Linux.
            "\u{e9}t\u{e9}.wav",
            "e\u{301}te\u{301}.wav",
            "drum \u{1F941} loop.wav",
            "kick #1 (copy) 100%.wav",
            "  spaced  .wav",
            "_under_score.wav",
            "kick.wav",
        ];
        let first = root.join(names[0]);
        write_wav(&first);
        let db = memory_db();
        let id = tagged(&db, root, &first, "drums");

        for pair in names.windows(2) {
            let (old, new) = (root.join(pair[0]), root.join(pair[1]));
            fs::rename(&old, &new).unwrap();
            apply(&db, &[root.to_path_buf()], &rename_event(&old, &new));
            assert_eq!(
                known(&db, root),
                std::slice::from_ref(&new),
                "{} → {}",
                pair[0],
                pair[1]
            );
            assert_eq!(db::file_id(&db, &new).unwrap(), Some(id));
        }
        assert_eq!(db::tags_of(&db, id).unwrap(), ["drums"]);
    }

    #[test]
    fn a_folder_with_a_like_wildcard_in_its_name_is_renamed_alone() {
        let dir = TempDir::new();
        let root = dir.path();
        let (wild, plain) = (root.join("a_b%"), root.join("aXbY"));
        write_wav(&wild.join("kick.wav"));
        write_wav(&plain.join("kick.wav"));
        let db = memory_db();
        index(&db, root, &mut |_| {});
        let new = root.join("renamed");
        fs::rename(&wild, &new).unwrap();

        apply(&db, &[root.to_path_buf()], &rename_event(&wild, &new));
        assert_eq!(
            known(&db, root),
            [plain.join("kick.wav"), new.join("kick.wav")]
        );
    }

    #[test]
    fn a_folder_moved_into_a_hidden_folder_is_forgotten() {
        let dir = TempDir::new();
        let root = dir.path();
        let (old, new) = (root.join("drums"), root.join(".Trash-1000/files/drums"));
        let snare = root.join("snare.wav");
        write_wav(&old.join("kick.wav"));
        write_wav(&snare);
        let db = memory_db();
        index(&db, root, &mut |_| {});
        fs::create_dir_all(new.parent().unwrap()).unwrap();
        fs::rename(&old, &new).unwrap();

        apply(&db, &[root.to_path_buf()], &rename_event(&old, &new));
        assert_eq!(known(&db, root), [snare]);
    }

    #[test]
    fn a_file_moved_out_of_a_hidden_folder_is_added() {
        let dir = TempDir::new();
        let root = dir.path();
        let (old, new) = (
            root.join(".Trash-1000/files/kick.wav"),
            root.join("kick.wav"),
        );
        write_wav(&old);
        let db = memory_db();
        index(&db, root, &mut |_| {});
        assert!(known(&db, root).is_empty());
        fs::rename(&old, &new).unwrap();

        apply(&db, &[root.to_path_buf()], &rename_event(&old, &new));
        assert_eq!(known(&db, root), [new]);
    }

    #[test]
    fn a_scanned_folder_that_is_itself_hidden_is_followed() {
        let dir = TempDir::new();
        let root = dir.path().join(".samples");
        let (old, new) = (root.join("kick.wav"), root.join("kick 2.wav"));
        write_wav(&old);
        let mut db = memory_db();
        let id = tagged(&db, &root, &old, "drums");
        let roots = [root.clone()];

        fs::rename(&old, &new).unwrap();
        apply(&db, &roots, &rename_event(&old, &new));
        let snare = root.join("snare.wav");
        write_wav(&snare);
        apply(
            &db,
            &roots,
            &event(EventKind::Create(CreateKind::File), &[&snare]),
        );
        catch_up(&mut db, &root, &mut ignore).unwrap();

        assert_eq!(known(&db, &root), [new.clone(), snare]);
        assert_eq!(db::file_id(&db, &new).unwrap(), Some(id));
    }

    #[test]
    fn a_hidden_folder_scanned_inside_another_scanned_folder_is_kept() {
        let dir = TempDir::new();
        let outer = dir.path().join("lib");
        let inner = outer.join(".private");
        let (kick, snare) = (outer.join("kick.wav"), inner.join("snare.wav"));
        write_wav(&kick);
        write_wav(&snare);
        let mut db = memory_db();
        let folders = [outer.clone(), inner.clone()];
        for folder in &folders {
            db::add_folder(&db, folder).unwrap();
            index(&db, folder, &mut |_| {});
        }
        // Both scanned by the user; the app catches up with `roots` only and
        // hands every scanned folder to `handle_event`, as `follow` and
        // `samplr watch` do.
        for root in &roots(&folders) {
            catch_up(&mut db, root, &mut ignore).unwrap();
        }
        let hat = inner.join("hat.wav");
        write_wav(&hat);
        apply(
            &db,
            &folders,
            &event(EventKind::Create(CreateKind::File), &[&hat]),
        );

        // Expected: what lies in a scanned folder is a sample, even when that
        // folder is hidden below another scanned folder.
        assert_eq!(known(&db, &outer), [inner.join("hat.wav"), snare, kick]);
    }

    #[test]
    fn a_rename_onto_a_known_sample_replaces_it() {
        let dir = TempDir::new();
        let root = dir.path();
        let (a, b) = (root.join("a.wav"), root.join("b.wav"));
        write_wav(&a);
        write_wav(&b);
        let db = memory_db();
        let a_id = tagged(&db, root, &a, "kept");
        let b_id = tagged(&db, root, &b, "overwritten");
        fs::rename(&a, &b).unwrap();

        apply(&db, &[root.to_path_buf()], &rename_event(&a, &b));
        assert_eq!(known(&db, root), std::slice::from_ref(&b));
        assert_eq!(db::file_id(&db, &b).unwrap(), Some(a_id));
        assert_eq!(db::tags_of(&db, a_id).unwrap(), ["kept"]);
        assert!(db::tags_of(&db, b_id).unwrap().is_empty());
    }

    #[test]
    fn the_same_rename_reported_twice_keeps_the_id() {
        let dir = TempDir::new();
        let root = dir.path();
        let (old, new) = (root.join("kick.wav"), root.join("kick 2.wav"));
        write_wav(&old);
        let db = memory_db();
        let id = tagged(&db, root, &old, "drums");
        fs::rename(&old, &new).unwrap();

        let renamed = rename_event(&old, &new);
        apply(&db, &[root.to_path_buf()], &renamed);
        apply(&db, &[root.to_path_buf()], &renamed);
        assert_eq!(known(&db, root), std::slice::from_ref(&new));
        assert_eq!(db::file_id(&db, &new).unwrap(), Some(id));
    }

    #[test]
    fn a_file_replaced_by_a_folder_of_the_same_name() {
        let dir = TempDir::new();
        let root = dir.path();
        let path = root.join("kick.wav");
        write_wav(&path);
        let db = memory_db();
        index(&db, root, &mut |_| {});
        fs::remove_file(&path).unwrap();
        let inside = path.join("hit.wav");
        write_wav(&inside);

        apply(
            &db,
            &[root.to_path_buf()],
            &event(EventKind::Remove(RemoveKind::File), &[&path]),
        );
        apply(
            &db,
            &[root.to_path_buf()],
            &event(EventKind::Create(CreateKind::Folder), &[&path]),
        );
        // Expected: the file's row is forgotten; it pointed at what is now a
        // folder, and a catch-up would keep it since the path still exists.
        assert_eq!(known(&db, root), [inside]);
    }

    #[test]
    fn a_folder_replaced_by_a_file_of_the_same_name() {
        let dir = TempDir::new();
        let root = dir.path();
        let path = root.join("drums");
        write_wav(&path.join("kick.wav"));
        let db = memory_db();
        index(&db, root, &mut |_| {});
        fs::remove_dir_all(&path).unwrap();
        fs::write(&path, b"not a folder").unwrap();

        apply(
            &db,
            &[root.to_path_buf()],
            &event(EventKind::Remove(RemoveKind::Folder), &[&path]),
        );
        apply(
            &db,
            &[root.to_path_buf()],
            &event(EventKind::Create(CreateKind::File), &[&path]),
        );
        assert!(known(&db, root).is_empty());
    }

    #[test]
    fn a_disk_mounted_inside_another_scanned_folder_unplugged_forgets_nothing() {
        let dir = TempDir::new();
        let outer = dir.path().join("lib");
        let disk = outer.join("external");
        let (kick, snare) = (outer.join("kick.wav"), disk.join("snare.wav"));
        write_wav(&kick);
        write_wav(&snare);
        let mut db = memory_db();
        let folders = [outer.clone(), disk.clone()];
        for folder in &folders {
            db::add_folder(&db, folder).unwrap();
            index(&db, folder, &mut |_| {});
        }
        fs::remove_dir_all(&disk).unwrap();

        apply(
            &db,
            &folders,
            &event(EventKind::Remove(RemoveKind::Folder), &[&disk]),
        );
        for root in &roots(&folders) {
            catch_up(&mut db, root, &mut ignore).unwrap();
        }
        // Expected: `external` is a scanned folder of its own; its going is
        // an unplugged disk, as for a scanned folder at the top.
        assert_eq!(known(&db, &outer), [disk.join("snare.wav"), kick]);
    }

    #[test]
    fn a_sample_in_a_folder_scanned_twice_is_saved_once() {
        let dir = TempDir::new();
        let outer = dir.path().join("lib");
        let inner = outer.join("drums");
        let kick = inner.join("kick.wav");
        write_wav(&kick);
        let db = memory_db();
        let id = tagged(&db, &outer, &kick, "drums");
        index(&db, &inner, &mut |_| {});

        let created = event(EventKind::Create(CreateKind::File), &[&kick]);
        // Even given every scanned folder rather than `roots`.
        apply(&db, &[outer.clone(), inner.clone()], &created);
        apply(&db, &[outer.clone(), inner.clone()], &created);
        assert_eq!(known(&db, &outer), std::slice::from_ref(&kick));
        assert_eq!(db::file_id(&db, &kick).unwrap(), Some(id));
        assert_eq!(db::tags_of(&db, id).unwrap(), ["drums"]);
    }

    #[test]
    fn an_event_outside_every_watched_folder_is_ignored() {
        let dir = TempDir::new();
        let root = dir.path().join("lib");
        fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("elsewhere/kick.wav");
        write_wav(&outside);
        let db = memory_db();

        for kind in [
            EventKind::Create(CreateKind::File),
            EventKind::Modify(ModifyKind::Any),
        ] {
            apply(&db, std::slice::from_ref(&root), &event(kind, &[&outside]));
        }
        // Expected: only the watched folders feed the library.
        assert!(known(&db, dir.path()).is_empty());
    }

    #[test]
    fn a_late_event_does_not_bring_back_a_removed_folder() {
        let dir = TempDir::new();
        let root = dir.path().join("lib");
        let kick = root.join("kick.wav");
        write_wav(&kick);
        let mut db = memory_db();
        db::add_folder(&db, &root).unwrap();
        index(&db, &root, &mut |_| {});
        crate::remove_folder(&mut db, &root).unwrap();
        assert!(is_hidden(&db, &kick));

        // A change queued before the folder stopped being watched, handled
        // once the watched folders no longer include it.
        apply(
            &db,
            &[],
            &event(EventKind::Modify(ModifyKind::Any), &[&kick]),
        );
        // Expected: the removed folder's samples stay hidden.
        assert!(is_hidden(&db, &kick));
    }

    #[test]
    fn a_rename_reported_from_one_end_only() {
        let dir = TempDir::new();
        let root = dir.path().join("lib");
        let kick = root.join("kick.wav");
        write_wav(&kick);
        let db = memory_db();
        index(&db, &root, &mut |_| {});
        let roots = [root.clone()];

        // Moved out of the watched folder: only its going is seen.
        let away = dir.path().join("kick.wav");
        fs::rename(&kick, &away).unwrap();
        apply(
            &db,
            &roots,
            &event(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                &[&kick],
            ),
        );
        assert!(known(&db, &root).is_empty());

        // Moved in from elsewhere, a file and a folder: only their coming.
        let back = root.join("back.wav");
        fs::rename(&away, &back).unwrap();
        apply(
            &db,
            &roots,
            &event(
                EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                &[&back],
            ),
        );
        let folder = dir.path().join("pack");
        write_wav(&folder.join("snare.wav"));
        let moved_in = root.join("pack");
        fs::rename(&folder, &moved_in).unwrap();
        apply(
            &db,
            &roots,
            &event(
                EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                &[&moved_in],
            ),
        );
        assert_eq!(known(&db, &root), [back, moved_in.join("snare.wav")]);
    }

    #[test]
    fn a_rename_whose_target_is_gone_by_then_leaves_nothing() {
        let dir = TempDir::new();
        let root = dir.path();
        let (old, new) = (root.join("kick.wav"), root.join("kick 2.wav"));
        write_wav(&old);
        let db = memory_db();
        index(&db, root, &mut |_| {});
        fs::rename(&old, &new).unwrap();
        fs::remove_file(&new).unwrap();

        apply(&db, &[root.to_path_buf()], &rename_event(&old, &new));
        apply(
            &db,
            &[root.to_path_buf()],
            &event(EventKind::Remove(RemoveKind::File), &[&new]),
        );
        assert!(known(&db, root).is_empty());
    }

    #[test]
    fn a_burst_on_one_path_ends_as_the_disk_does() {
        let dir = TempDir::new();
        let root = dir.path();
        let kick = root.join("kick.wav");
        write_wav(&kick);
        let db = memory_db();
        let id = tagged(&db, root, &kick, "drums");
        let roots = [root.to_path_buf()];
        let burst = [
            event(EventKind::Remove(RemoveKind::File), &[&kick]),
            event(EventKind::Create(CreateKind::File), &[&kick]),
            event(EventKind::Remove(RemoveKind::File), &[&kick]),
            event(EventKind::Create(CreateKind::File), &[&kick]),
        ];

        // Saved again in place (an editor's atomic save): still on disk at
        // the end, so the row and its tags stay.
        for event in &burst {
            apply(&db, &roots, event);
        }
        assert_eq!(db::file_id(&db, &kick).unwrap(), Some(id));
        assert_eq!(db::tags_of(&db, id).unwrap(), ["drums"]);

        fs::remove_file(&kick).unwrap();
        for event in &burst {
            apply(&db, &roots, event);
        }
        assert!(known(&db, root).is_empty());
    }

    #[test]
    fn empty_and_non_audio_files_are_not_samples() {
        let dir = TempDir::new();
        let root = dir.path();
        let (empty, text, fake) = (
            root.join("empty.wav"),
            root.join("readme.txt"),
            root.join("fake.wav"),
        );
        fs::write(&empty, b"").unwrap();
        fs::write(&text, b"hello").unwrap();
        fs::write(&fake, b"not a sound at all").unwrap();
        let db = memory_db();
        let roots = [root.to_path_buf()];

        for path in [&empty, &text, &fake] {
            apply(
                &db,
                &roots,
                &event(EventKind::Create(CreateKind::File), &[path]),
            );
        }
        assert!(known(&db, root).is_empty());

        // The copy ends: the empty file now holds a sound.
        write_wav(&empty);
        apply(
            &db,
            &roots,
            &event(EventKind::Modify(ModifyKind::Any), &[&empty]),
        );
        assert_eq!(known(&db, root), [empty]);
    }

    #[test]
    fn a_sample_emptied_on_disk_keeps_its_row() {
        let dir = TempDir::new();
        let root = dir.path();
        let kick = root.join("kick.wav");
        write_wav(&kick);
        let db = memory_db();
        let id = tagged(&db, root, &kick, "drums");
        // Truncated by a program about to write it again.
        fs::write(&kick, b"").unwrap();

        apply(
            &db,
            &[root.to_path_buf()],
            &event(EventKind::Modify(ModifyKind::Any), &[&kick]),
        );
        assert_eq!(db::file_id(&db, &kick).unwrap(), Some(id));
    }

    #[test]
    fn access_events_change_nothing() {
        let dir = TempDir::new();
        let root = dir.path();
        let kick = root.join("kick.wav");
        write_wav(&kick);
        let db = memory_db();
        let access = notify_debouncer_full::notify::event::AccessKind::Any;
        apply(
            &db,
            &[root.to_path_buf()],
            &event(EventKind::Access(access), &[&kick]),
        );
        assert!(known(&db, root).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn catching_up_keeps_samples_of_an_unreadable_folder() {
        let dir = TempDir::new();
        let root = dir.path();
        let (locked, kick) = (root.join("locked"), root.join("locked/kick.wav"));
        let snare = root.join("snare.wav");
        write_wav(&kick);
        write_wav(&snare);
        let mut db = memory_db();
        let id = tagged(&db, root, &kick, "drums");
        let Some(_lock) = Locked::new(&locked) else {
            return;
        };

        catch_up(&mut db, root, &mut ignore).unwrap();
        // Expected: a sample that cannot be reached is not proven gone, and
        // forgetting it loses its tags (as with an unplugged disk).
        assert_eq!(db::file_id(&db, &kick).unwrap(), Some(id));
        assert_eq!(known(&db, root), [kick, snare]);
    }

    // inotify refuses a folder holding one it cannot read; FSEvents, on a
    // Mac, watches it all the same.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_folder_that_cannot_be_watched_leaves_the_others_followed() {
        let dir = TempDir::new();
        let (refused, followed) = (dir.path().join("refused"), dir.path().join("followed"));
        write_wav(&refused.join("locked/kick.wav"));
        fs::create_dir_all(&followed).unwrap();
        let Some(_lock) = Locked::new(&refused.join("locked")) else {
            return;
        };
        let (mut watcher, events) = new_watcher().unwrap();

        assert!(watcher.watch(&refused, RecursiveMode::Recursive).is_err());
        watcher.watch(&followed, RecursiveMode::Recursive).unwrap();

        let db = memory_db();
        let roots = [followed.clone()];
        let snare = followed.join("snare.wav");
        write_wav(&snare);
        let deadline = Instant::now() + Duration::from_secs(20);
        while known(&db, &followed).is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "the followed folder's change never came");
            if let Ok(Ok(batch)) = events.recv_timeout(left) {
                for event in &batch {
                    apply(&db, &roots, event);
                }
            }
        }
        assert_eq!(known(&db, &followed), [snare]);
    }
}
