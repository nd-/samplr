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
    /// What went wrong while following the folders, not yet shown to the
    /// user: the page takes them with the `library_problems` command.
    problems: Mutex<Vec<String>>,
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
            problems: Mutex::new(Vec::new()),
        }
    }

    /// Keeps a problem for the page; following goes on regardless.
    fn problem(&self, message: String) {
        eprintln!("{message}");
        if let Ok(mut problems) = self.problems.lock() {
            problems.push(message);
        }
    }

    /// The problems not yet shown, emptied.
    pub fn take_problems(&self) -> Vec<String> {
        match self.problems.lock() {
            Ok(mut problems) => std::mem::take(&mut *problems),
            Err(_) => Vec::new(),
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
    /// newly watched. A folder that cannot be watched (too many folders for
    /// the system, no permission) is reported as a problem and left out, the
    /// others are watched all the same; it is tried again at the next call.
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
        let mut watched = Vec::new();
        for root in wanted {
            if watching.roots.contains(&root) {
                watched.push(root);
                continue;
            }
            match watching.watcher.watch(&root, RecursiveMode::Recursive) {
                Ok(()) => {
                    added.push(root.clone());
                    watched.push(root);
                }
                Err(error) => self.problem(format!(
                    "{}: changes in this folder are not followed ({error})",
                    root.display()
                )),
            }
        }
        watching.roots = watched;
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
            app.state::<Library>()
                .problem(format!("The folders are not followed: {error}"));
            tell_problems(&app, &app.state::<Library>());
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
    // A folder that cannot be caught up with is reported, the others still
    // are, and all are followed live afterwards.
    for root in library.update_watched()? {
        if let Err(error) = watch::catch_up(&mut db, &root, &mut note) {
            library.problem(format!(
                "{}: changes made while samplr was closed were not applied ({error})",
                root.display()
            ));
        }
    }
    if changed {
        tell_page(app);
    }
    tell_problems(app, &library);
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
                        let paths: Vec<String> = event
                            .paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect();
                        library.problem(format!(
                            "A change on disk was not applied ({}): {error}",
                            paths.join(", ")
                        ));
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
        tell_problems(app, &library);
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

/// Tells the page there are problems to show, if any. The page also asks at
/// startup: problems met before it was listening are not lost.
fn tell_problems(app: &AppHandle, library: &Library) {
    let pending = library
        .problems
        .lock()
        .map(|p| !p.is_empty())
        .unwrap_or(false);
    if pending {
        let _ = app.emit("library-problem", ());
    }
}
