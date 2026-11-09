// Follows the scanned folders while the app is open, like `samplr watch`:
// samples added, renamed (tags kept) or deleted on disk show up in the list.
// The page is told with a `library-changed` event and searches again.
// New samples get their waveform computed in the background, so that it shows
// at once when they are selected.

use std::error::Error;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use samplr::watch::{self, RecursiveMode, Watcher};
use samplr::{WaveformUpdate, db};
use tauri::{AppHandle, Emitter, Manager};

/// Shared through `State`: the watcher and the folders it watches, so that
/// `add_folder` can watch a new folder at once.
pub struct Library {
    watching: Mutex<Option<Watching>>,
    /// Wakes up the thread that computes the missing waveforms.
    waveforms: Sender<()>,
}

struct Watching {
    watcher: Watcher,
    roots: Vec<PathBuf>,
}

impl Library {
    pub fn new() -> Self {
        let (waveforms, wake_ups) = mpsc::channel();
        thread::spawn(move || compute_waveforms(wake_ups));
        Library {
            watching: Mutex::new(None),
            waveforms,
        }
    }

    /// Asks for the missing or stale waveforms to be computed, in the
    /// background.
    pub fn update_waveforms(&self) {
        // Fails only if the thread has stopped, when the app closes.
        let _ = self.waveforms.send(());
    }

    /// Watches the folders of the database that are not watched yet, and
    /// stops watching those now inside another one. Returns the folders
    /// newly watched.
    pub fn update_watched(&self) -> Result<Vec<PathBuf>, Box<dyn Error>> {
        let db = db::open()?;
        // An unplugged disk cannot be watched: it is watched again at the
        // next start, or when its folder is added again.
        let wanted: Vec<PathBuf> = watch::roots(&db::folders(&db)?)
            .into_iter()
            .filter(|root| root.is_dir())
            .collect();

        let mut guard = self.watching.lock().map_err(|_| "watcher unavailable")?;
        let Some(watching) = guard.as_mut() else {
            return Ok(Vec::new());
        };
        for root in &watching.roots {
            if !wanted.contains(root) {
                // Already gone if the folder was deleted: nothing to undo.
                let _ = watching.watcher.unwatch(root);
            }
        }
        let mut added = Vec::new();
        for root in &wanted {
            if !watching.roots.contains(root) {
                watching.watcher.watch(root, RecursiveMode::Recursive)?;
                added.push(root.clone());
            }
        }
        watching.roots = wanted;
        Ok(added)
    }

    fn roots(&self) -> Vec<PathBuf> {
        match self.watching.lock() {
            Ok(guard) => guard.as_ref().map(|w| w.roots.clone()).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }
}

/// Starts following the folders, in a thread of its own: first catches up
/// with what changed while the app was closed, then applies the changes as
/// they come.
pub fn start(app: AppHandle) {
    thread::spawn(move || {
        if let Err(error) = follow(&app) {
            eprintln!("Folders not followed: {error}");
        }
    });
}

fn follow(app: &AppHandle) -> Result<(), Box<dyn Error>> {
    let library = app.state::<Library>();
    let (watcher, events) = watch::new_watcher()?;
    *library.watching.lock().map_err(|_| "watcher unavailable")? = Some(Watching {
        watcher,
        roots: Vec::new(),
    });

    // Watching starts before catching up: a file added meanwhile cannot
    // slip between the two.
    let mut db = db::open()?;
    let mut changed = false;
    let mut note = |_: String| changed = true;
    for root in library.update_watched()? {
        watch::catch_up(&mut db, &root, &mut note)?;
    }
    if changed {
        tell_page(app);
    }
    library.update_waveforms();

    // Ends when the watcher is dropped, that is when the app closes.
    for batch in events {
        let mut changed = false;
        let mut note = |_: String| changed = true;
        match batch {
            Ok(batch) => {
                // Read at each batch: `add_folder` may have added a folder.
                let roots = library.roots();
                for event in &batch {
                    if let Err(error) = watch::handle_event(&db, &roots, event, &mut note) {
                        eprintln!("Error: {error}");
                    }
                }
            }
            Err(errors) => {
                for error in errors {
                    eprintln!("Watch error: {error}");
                }
            }
        }
        if changed {
            tell_page(app);
            library.update_waveforms();
        }
    }
    Ok(())
}

/// Runs for the whole session: at each wake-up, computes every missing or
/// stale waveform, like `samplr waveforms`.
fn compute_waveforms(wake_ups: Receiver<()>) {
    // Ends when the app closes.
    while wake_ups.recv().is_ok() {
        // Wake-ups that came during the last pass need only one more pass.
        while wake_ups.try_recv().is_ok() {}
        if let Err(error) = update_all_waveforms() {
            eprintln!("Waveforms: {error}");
        }
    }
}

fn update_all_waveforms() -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    for (file_id, path) in db::all_files(&db)? {
        if let WaveformUpdate::Unreadable(error) = samplr::update_waveform(&db, file_id, &path)? {
            eprintln!("{}: {error}", path.display());
        }
    }
    Ok(())
}

fn tell_page(app: &AppHandle) {
    // Fails only if the window is already closed: nothing to tell then.
    let _ = app.emit("library-changed", ());
}
