// samplr: scan sample folders, tag the samples, find them again and listen to them.
//
// Examples:
//   cargo run -- scan ~/Samples              → adds the folder's samples to the database
//   cargo run -- tag ~/Samples/kick.wav 808  → adds the "808" tag to a sample
//   cargo run -- search 808 --path kick      → samples tagged 808 whose path contains "kick"
//   cargo run -- search 808 --json           → the same, as JSON, for scripts
//   cargo run -- help                        → every command

use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
// The shared code lives in the `samplr` library (src/lib.rs), which the
// graphical interface will use too.
use samplr::db;
use samplr::{SampleInfo, WaveformUpdate, is_audio, is_mac_metadata, read_info, waveform};

// clap reads this description to parse the command line: each variant of
// `Command` becomes a subcommand (`Scan` → `samplr scan`), its fields become
// the arguments, and the `///` comments become the help shown by `samplr help`.

/// Audio sample manager: scan, tag, search, listen.
#[derive(Parser)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Command,
    /// Prints the result as JSON, for scripts (`watch`: one line per change)
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Adds the samples of a folder and its subfolders to the database
    Scan { folder: PathBuf },
    /// Shows every sample in the database, with its tags
    List,
    /// Lists the scanned folders and how many samples each holds
    Folders,
    /// Removes a scanned folder: no longer followed, its samples leave the
    /// list. Their tags are kept and come back if the folder is scanned
    /// again. The audio files are not touched
    RemoveFolder { folder: PathBuf },
    /// Follows the scanned folders and updates the database live (Ctrl+C to stop)
    Watch,
    /// Plays a file on the default audio output
    Play { file: PathBuf },
    /// Adds tags to a sample (and adds the sample to the database if needed)
    Tag {
        file: PathBuf,
        #[arg(required = true)]
        tags: Vec<String>,
    },
    /// Marks samples as favorites (and adds them to the database if needed)
    Fav {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Removes samples from the favorites
    Unfav {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Removes tags from a sample
    Untag {
        file: PathBuf,
        #[arg(required = true)]
        tags: Vec<String>,
    },
    /// Lists the existing tags and how many samples carry each
    Tags,
    /// Draws a sample's waveform in the terminal
    Wave {
        file: PathBuf,
        /// Width of the drawing, in characters
        #[arg(long, default_value_t = 80)]
        width: usize,
    },
    /// Computes the waveforms missing from the database, or out of date
    Waveforms,
    /// Finds samples, with the syntax of the interface's search box
    Search {
        /// Each word must be in the path or a tag; '#tag' an exact tag
        /// (quoted: the shell takes '#' for a comment); filters bpm:120,
        /// bpm:118-124, key:Am, loop:yes, type:kick
        words: Vec<String>,
        /// Only keep the favorites
        #[arg(long)]
        fav: bool,
    },
    /// Finds the samples that sound most like a sample of the library (its
    /// timbre: brightness, noise, attack…), nearest first
    Similar {
        file: PathBuf,
        /// How many to show
        #[arg(short = 'n', long, default_value_t = 20)]
        count: usize,
    },
}

fn main() {
    // On invalid arguments, clap prints the help and stops the program.
    let args = Args::parse();

    // `match` compares a value against several patterns, a bit like a switch.
    // Each pattern "opens" the variant and names its fields.
    let json = args.json;
    let result = match args.command {
        Command::Scan { folder } => scan(&folder, json),
        Command::List => search(&[], false, json),
        Command::Folders => list_folders(json),
        Command::RemoveFolder { folder } => remove_folder(&folder, json),
        Command::Watch => watch(json),
        Command::Play { file } => play(&file, json),
        Command::Tag { file, tags } => tag(&file, &tags, json),
        Command::Fav { files } => set_favorite(&files, true, json),
        Command::Unfav { files } => set_favorite(&files, false, json),
        Command::Untag { file, tags } => untag(&file, &tags, json),
        Command::Tags => list_tags(json),
        Command::Search { words, fav } => search(&words, fav, json),
        Command::Wave { file, width } => wave(&file, width, json),
        Command::Waveforms => compute_waveforms(json),
        Command::Similar { file, count } => similar(&file, count, json),
    };

    // If a function returned an error, print it cleanly.
    if let Err(error) = result {
        // `samplr list | head`: `head` closes the pipe as soon as it has its
        // lines, and the next write fails ("broken pipe"). Not samplr's
        // fault: stop silently, like `ls` or `grep`. `downcast_ref` checks
        // whether the error, of unknown type here, is an I/O error.
        if let Some(io_error) = error.downcast_ref::<io::Error>()
            && io_error.kind() == io::ErrorKind::BrokenPipe
        {
            process::exit(0);
        }
        eprintln!("Error: {error}");
        process::exit(1);
    }
}

/// Saves the folder and the samples it contains to the database, then shows
/// them as a table.
fn scan(folder: &Path, json: bool) -> Result<(), Box<dyn Error>> {
    let mut db = db::open()?;
    let samplr::Scan {
        folder,
        samples,
        unreadable,
    } = samplr::scan(&mut db, folder, &mut |_, _| {})?;
    if json {
        return print_json(&json!({
            "folder": folder,
            "samples": samples,
            "unreadable": unreadable,
        }));
    }

    // The output is only a report: if it is closed (`samplr scan … | head`),
    // write failures are ignored (`let _ =`).
    let mut out = io::stdout().lock();
    let _ = print_header(&mut out);
    for info in &samples {
        // Path shown relative to the scanned folder, easier to read.
        let relative = info.path.strip_prefix(&folder).unwrap_or(&info.path);
        let _ = print_row(&mut out, info, relative);
    }
    let _ = writeln!(
        out,
        "\n{} sample(s) saved, {unreadable} unreadable.",
        samples.len()
    );
    Ok(())
}

/// Catches up with the changes made while samplr was not running, then
/// follows the scanned folders live until Ctrl+C.
fn watch(json: bool) -> Result<(), Box<dyn Error>> {
    let mut db = db::open()?;
    let roots = samplr::watch::roots(&db::folders(&db)?);
    if roots.is_empty() {
        return Err("no folder to watch: run \"samplr scan <folder>\" first".into());
    }

    let (mut watcher, receiver) = samplr::watch::new_watcher()?;
    // As JSON, one object per line, so that a script reads them as they come.
    let mut print = |message: String| {
        if json {
            println!("{}", json!({ "change": message }));
        } else {
            println!("{message}");
        }
    };
    let mut watched = 0;
    for root in &roots {
        if !root.is_dir() {
            eprintln!(
                "{} not found (disk unplugged?): not watched",
                root.display()
            );
            continue;
        }
        // Watching starts before catching up: a file added meanwhile cannot
        // slip between the two.
        watcher.watch(root, samplr::watch::RecursiveMode::Recursive)?;
        samplr::watch::catch_up(&mut db, root, &mut print)?;
        watched += 1;
    }
    if watched == 0 {
        return Err("none of the scanned folders is available".into());
    }

    if !json {
        println!("Watching. Ctrl+C to stop.");
    }
    // The loop waits for the next batch of events; it never ends on its
    // own, Ctrl+C stops the program.
    for batch in receiver {
        match batch {
            Ok(events) => {
                for event in &events {
                    if let Err(error) = samplr::watch::handle_event(&db, &roots, event, &mut print)
                    {
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
    }
    Ok(())
}

fn list_folders(json: bool) -> Result<(), Box<dyn Error>> {
    let folders = samplr::folders(&db::open()?)?;
    if json {
        return print_json(&folders);
    }
    let mut out = io::stdout().lock();
    for folder in folders {
        let missing = if folder.found { "" } else { "  (not found)" };
        writeln!(
            out,
            "{:>6}  {}{missing}",
            folder.samples,
            folder.path.display()
        )?;
    }
    Ok(())
}

fn remove_folder(folder: &Path, json: bool) -> Result<(), Box<dyn Error>> {
    let mut db = db::open()?;
    let hidden = samplr::remove_folder(&mut db, folder)?;
    if json {
        return print_json(&json!({ "folder": folder, "hidden": hidden }));
    }
    println!("Folder removed, {hidden} sample(s) hidden (tags kept).");
    Ok(())
}

/// Shows the samples matching `words`, read as the interface's search box
/// reads them (see `samplr::query`); with no word, every sample.
fn search(words: &[String], favorites_only: bool, json: bool) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let samples = samplr::query::search(&db, &words.join(" "), favorites_only, None)?;
    if json {
        return print_json(&samples);
    }

    let mut out = io::stdout().lock();
    print_header(&mut out)?;
    for info in &samples {
        print_row(&mut out, info, &info.path)?;
    }
    writeln!(out, "\n{} sample(s) found.", samples.len())?;
    Ok(())
}

/// The samples nearest a sample's timbre, as a table with their distance.
fn similar(file: &Path, count: usize, json: bool) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let file = dunce::canonicalize(file)?;
    let file_id = db::require_file_id(&db, &file)?;
    let ranked = samplr::similar(&db, file_id, &file)?;
    let mut samples: HashMap<PathBuf, SampleInfo> = samplr::query::search(&db, "", false, None)?
        .into_iter()
        .map(|info| (info.path.clone(), info))
        .collect();
    let nearest: Vec<(f32, SampleInfo)> = ranked
        .into_iter()
        .filter_map(|(path, distance)| samples.remove(&path).map(|info| (distance, info)))
        .take(count)
        .collect();
    if json {
        let mut list = Vec::new();
        for (distance, info) in &nearest {
            let mut value = serde_json::to_value(info)?;
            value["distance"] = json!(distance);
            list.push(value);
        }
        return print_json(&json!({ "file": file, "similar": list }));
    }

    let mut out = io::stdout().lock();
    writeln!(out, "Sounding like {}:\n", file.display())?;
    write!(out, "{:>5}  ", "DIST.")?;
    print_header(&mut out)?;
    for (distance, info) in &nearest {
        write!(out, "{distance:>5.2}  ")?;
        print_row(&mut out, info, &info.path)?;
    }
    Ok(())
}

/// Adds tags to a sample. The file is read again every time: if it was not in
/// the database yet, it gets added; otherwise its info is refreshed.
fn tag(file: &Path, tags: &[String], json: bool) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let (file, file_id) = ensure_saved(&db, file)?;
    let added = db::add_tags(&db, file_id, tags)?;
    if json {
        return print_json(&json!({ "file": file, "added": added }));
    }

    println!("{added} tag(s) added to {}", file.display());
    Ok(())
}

/// Marks samples as favorites, or no longer. Like `tag`, adds a file to the
/// database if it was not there yet; `unfav` only needs it to be there.
fn set_favorite(files: &[PathBuf], favorite: bool, json: bool) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let mut changed = Vec::new();
    for file in files {
        let (file, file_id) = if favorite {
            ensure_saved(&db, file)?
        } else {
            let file = dunce::canonicalize(file)?;
            let file_id = db::require_file_id(&db, &file)?;
            (file, file_id)
        };
        db::set_favorite(&db, file_id, favorite)?;
        if json {
            changed.push(json!({ "file": file, "favorite": favorite }));
            continue;
        }
        let state = if favorite {
            "★"
        } else {
            "no longer a favorite:"
        };
        println!("{state} {}", file.display());
    }
    if json {
        return print_json(&changed);
    }
    Ok(())
}

/// Makes sure an audio file is in the database, and returns its absolute
/// path and its `id`. The file is read again every time: if it was not in the
/// database yet, it gets added; otherwise its info is refreshed.
fn ensure_saved(db: &Connection, file: &Path) -> Result<(PathBuf, i64), Box<dyn Error>> {
    let file = dunce::canonicalize(file)?;
    if !is_audio(&file) {
        return Err(format!("{} is not a known audio file", file.display()).into());
    }
    let info = read_info(&file)?;
    let file_id = db::save(db, &info)?;
    Ok((file, file_id))
}

/// Draws a sample's waveform in the terminal, computing it first if it is
/// not in the database or out of date.
fn wave(file: &Path, width: usize, json: bool) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let (file, file_id) = ensure_saved(&db, file)?;
    let peaks = samplr::peaks(&db, file_id, &file)?;
    if json {
        // The cached peaks, lowest and highest value (-1 to 1) of each slice
        // of the file; `--width` only concerns the drawing.
        return print_json(&json!({ "file": file, "peaks": peaks }));
    }

    print!("{}", waveform::draw(&peaks, width, 6));
    println!("{}", file.display());
    Ok(())
}

/// Computes every waveform missing from the database or out of date.
fn compute_waveforms(json: bool) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let (mut computed, mut up_to_date, mut unreadable, mut missing) = (0, 0, 0, 0);
    let mut errors = Vec::new();

    // No transaction here, unlike `scan`: decoding takes far longer than
    // writing, and each waveform saved right away is kept even if the
    // command is stopped halfway with Ctrl+C.
    for (file_id, path) in db::all_files(&db)? {
        match samplr::update_waveform(&db, file_id, &path)? {
            WaveformUpdate::Computed => {
                computed += 1;
                if !json {
                    println!("+ {}", path.display());
                }
            }
            WaveformUpdate::UpToDate => up_to_date += 1,
            WaveformUpdate::Missing => missing += 1,
            WaveformUpdate::Unreadable(error) => {
                if json {
                    errors.push(json!({ "file": path, "error": error }));
                } else {
                    eprintln!("{}: {error}", path.display());
                }
                unreadable += 1;
            }
        }
    }
    if json {
        return print_json(&json!({
            "computed": computed,
            "up_to_date": up_to_date,
            "unreadable": unreadable,
            "missing": missing,
            "errors": errors,
        }));
    }

    println!(
        "\n{computed} computed, {up_to_date} already up to date, {unreadable} unreadable, {missing} missing."
    );
    Ok(())
}

fn untag(file: &Path, tags: &[String], json: bool) -> Result<(), Box<dyn Error>> {
    let file = dunce::canonicalize(file)?;

    let db = db::open()?;
    let file_id = db::require_file_id(&db, &file)?;
    let removed = db::remove_tags(&db, file_id, tags)?;
    if json {
        return print_json(&json!({ "file": file, "removed": removed }));
    }

    println!("{removed} tag(s) removed from {}", file.display());
    Ok(())
}

fn list_tags(json: bool) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let tags = db::list_tags(&db)?;
    if json {
        let tags: Vec<_> = tags
            .iter()
            .map(|(name, count)| json!({ "name": name, "count": count }))
            .collect();
        return print_json(&tags);
    }

    let mut out = io::stdout().lock();
    for (name, count) in &tags {
        writeln!(out, "{count:>5}  {name}")?;
    }
    writeln!(out, "\n{} tag(s).", tags.len())?;
    Ok(())
}

/// Prints a value as JSON on its own line. Written through `writeln!` like
/// the tables, so that a closed pipe (`samplr list --json | head`) is an
/// I/O error, which `main` ignores.
fn print_json(value: &impl Serialize) -> Result<(), Box<dyn Error>> {
    let text = serde_json::to_string_pretty(value)?;
    writeln!(io::stdout().lock(), "{text}")?;
    Ok(())
}

// The display functions write with `writeln!` into the `out` they receive
// rather than with `println!`: `println!` stops the program on a write error,
// `writeln!` returns it and lets the caller decide.

fn print_header(out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "{:>8}  {:>7}  {:>4}  {:>5}  FILE",
        "LENGTH", "RATE", "BITS", "CHAN."
    )
}

/// One table row; `path` is the path to show, full or shortened.
/// The tags, if any, follow the path between brackets.
fn print_row(out: &mut impl Write, info: &SampleInfo, path: &Path) -> io::Result<()> {
    let tags = if info.tags.is_empty() {
        String::new()
    } else {
        format!("  [{}]", info.tags.join(" "))
    };
    let star = if info.favorite { "★ " } else { "" };
    writeln!(
        out,
        "{:>8}  {:>7}  {:>4}  {:>5}  {star}{}{tags}",
        format_duration(info.duration_secs),
        info.sample_rate
            .map(|r| format!("{r}"))
            .unwrap_or("?".into()),
        info.bits.map(|b| format!("{b}")).unwrap_or("-".into()),
        info.channels.map(|c| format!("{c}")).unwrap_or("?".into()),
        path.display()
    )
}

/// 1.234 → "1.23 s", 75.0 → "1:15"
fn format_duration(duration: Option<f64>) -> String {
    match duration {
        Some(s) if s < 60.0 => format!("{s:.2} s"),
        Some(s) => format!("{}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64),
        None => "?".to_string(),
    }
}

/// Plays a file on the default audio output and waits until it ends.
fn play(path: &Path, json: bool) -> Result<(), Box<dyn Error>> {
    if is_mac_metadata(path) {
        return Err(format!("{}: macOS metadata, not an audio file", path.display()).into());
    }

    // Some files open without error but produce no sample at all (Apple
    // `.m4r` ringtones). This is checked on a throwaway decoder: reading the
    // first sample on the one that plays would consume it, and would swap
    // the left and right channels of a stereo file.
    let file = File::open(path)?;
    let mut probe = samplr::unless_it_panics(|| Ok(rodio::Decoder::try_from(file)?))?;
    if probe.next().is_none() {
        return Err("no sound decoded (format recognised, but content unreadable)".into());
    }

    // Opens the default audio output (CoreAudio on Mac, ALSA on Linux).
    let mut sink = rodio::DeviceSinkBuilder::open_default_sink()?;
    sink.log_on_drop(false); // avoids a stray message on close

    let player = rodio::Player::connect_new(sink.mixer());
    let file = File::open(path)?;
    player.append(rodio::Decoder::try_from(file)?);

    if !json {
        println!("▶ Playing {}", path.display());
    }
    player.sleep_until_end();
    // `sleep_until_end` returns once rodio has handed over every sample, not
    // once they have left the speakers: some are still in the sound card's
    // buffer. Closing the output right away would drop them, and a very
    // short sample (a 75 ms kick) would vanish entirely.
    thread::sleep(Duration::from_millis(300));
    if json {
        return print_json(&json!({ "played": path }));
    }
    Ok(())
}
