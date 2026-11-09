// samplr: scan sample folders, tag the samples, find them again and listen to them.
//
// Examples:
//   cargo run -- scan ~/Samples              → adds the folder's samples to the database
//   cargo run -- tag ~/Samples/kick.wav 808  → adds the "808" tag to a sample
//   cargo run -- search 808 --path kick      → samples tagged 808 whose path contains "kick"
//   cargo run -- help                        → every command

use std::error::Error;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use rusqlite::Connection;
// The shared code lives in the `samplr` library (src/lib.rs), which the
// graphical interface will use too.
use samplr::db::{self, FileStamp};
use samplr::{SampleInfo, is_audio, is_mac_metadata, read_info, waveform};

// clap reads this description to parse the command line: each variant of
// `Command` becomes a subcommand (`Scan` → `samplr scan`), its fields become
// the arguments, and the `///` comments become the help shown by `samplr help`.

/// Audio sample manager: scan, tag, search, listen.
#[derive(Parser)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Adds the samples of a folder and its subfolders to the database
    Scan { folder: PathBuf },
    /// Shows every sample in the database, with its tags
    List,
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
    /// Finds the samples carrying all the given tags
    Search {
        tags: Vec<String>,
        /// Only keep the samples whose path contains this text
        #[arg(long)]
        path: Option<String>,
    },
}

fn main() {
    // On invalid arguments, clap prints the help and stops the program.
    let args = Args::parse();

    // `match` compares a value against several patterns, a bit like a switch.
    // Each pattern "opens" the variant and names its fields.
    let result = match args.command {
        Command::Scan { folder } => scan(&folder),
        Command::List => search(&[], None),
        Command::Watch => watch(),
        Command::Play { file } => play(&file),
        Command::Tag { file, tags } => tag(&file, &tags),
        Command::Untag { file, tags } => untag(&file, &tags),
        Command::Tags => list_tags(),
        Command::Search { tags, path } => search(&tags, path.as_deref()),
        Command::Wave { file, width } => wave(&file, width),
        Command::Waveforms => compute_waveforms(),
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
fn scan(folder: &Path) -> Result<(), Box<dyn Error>> {
    let mut db = db::open()?;
    let samplr::Scan {
        folder,
        samples,
        unreadable,
    } = samplr::scan(&mut db, folder)?;

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
fn watch() -> Result<(), Box<dyn Error>> {
    let mut db = db::open()?;
    let roots = samplr::watch::roots(&db::folders(&db)?);
    if roots.is_empty() {
        return Err("no folder to watch: run \"samplr scan <folder>\" first".into());
    }

    let (mut watcher, receiver) = samplr::watch::new_watcher()?;
    let mut print = |message: String| println!("{message}");
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

    println!("Watching. Ctrl+C to stop.");
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

/// Shows the samples in the database carrying all the `tags` and whose path
/// contains `path`; with no tag and no path, every sample.
fn search(tags: &[String], path: Option<&str>) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let samples = db::search(&db, tags, &[], path.unwrap_or(""))?;

    let mut out = io::stdout().lock();
    print_header(&mut out)?;
    for info in &samples {
        print_row(&mut out, info, &info.path)?;
    }
    writeln!(out, "\n{} sample(s) found.", samples.len())?;
    Ok(())
}

/// Adds tags to a sample. The file is read again every time: if it was not in
/// the database yet, it gets added; otherwise its info is refreshed.
fn tag(file: &Path, tags: &[String]) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let (file, file_id) = ensure_saved(&db, file)?;
    let added = db::add_tags(&db, file_id, tags)?;

    println!("{added} tag(s) added to {}", file.display());
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
fn wave(file: &Path, width: usize) -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let (file, file_id) = ensure_saved(&db, file)?;
    let peaks = samplr::peaks(&db, file_id, &file)?;

    print!("{}", waveform::draw(&peaks, width, 6));
    println!("{}", file.display());
    Ok(())
}

/// Computes every waveform missing from the database or out of date.
fn compute_waveforms() -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let (mut computed, mut up_to_date, mut unreadable, mut missing) = (0, 0, 0, 0);

    // No transaction here, unlike `scan`: decoding takes far longer than
    // writing, and each waveform saved right away is kept even if the
    // command is stopped halfway with Ctrl+C.
    for (file_id, path) in db::all_files(&db)? {
        // The stamp is read before decoding: if the file changes meanwhile,
        // the saved stamp will no longer match and the waveform will be
        // computed again next time.
        let Ok(stamp) = FileStamp::read(&path) else {
            // Deleted file, or disk unplugged.
            missing += 1;
            continue;
        };
        if db::waveform(&db, file_id, stamp)?.is_some() {
            up_to_date += 1;
            continue;
        }
        match waveform::compute(&path) {
            Ok(peaks) => {
                db::save_waveform(&db, file_id, stamp, &peaks)?;
                computed += 1;
                println!("+ {}", path.display());
            }
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                unreadable += 1;
            }
        }
    }

    println!(
        "\n{computed} computed, {up_to_date} already up to date, {unreadable} unreadable, {missing} missing."
    );
    Ok(())
}

fn untag(file: &Path, tags: &[String]) -> Result<(), Box<dyn Error>> {
    let file = dunce::canonicalize(file)?;

    let db = db::open()?;
    let file_id = db::file_id(&db, &file)?
        .ok_or_else(|| format!("{} is not in the database", file.display()))?;
    let removed = db::remove_tags(&db, file_id, tags)?;

    println!("{removed} tag(s) removed from {}", file.display());
    Ok(())
}

fn list_tags() -> Result<(), Box<dyn Error>> {
    let db = db::open()?;
    let tags = db::list_tags(&db)?;

    let mut out = io::stdout().lock();
    for (name, count) in &tags {
        writeln!(out, "{count:>5}  {name}")?;
    }
    writeln!(out, "\n{} tag(s).", tags.len())?;
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
    writeln!(
        out,
        "{:>8}  {:>7}  {:>4}  {:>5}  {}{tags}",
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
fn play(path: &Path) -> Result<(), Box<dyn Error>> {
    if is_mac_metadata(path) {
        return Err(format!("{}: macOS metadata, not an audio file", path.display()).into());
    }

    // Some files open without error but produce no sample at all (Apple
    // `.m4r` ringtones). This is checked on a throwaway decoder: reading the
    // first sample on the one that plays would consume it, and would swap
    // the left and right channels of a stereo file.
    let mut probe = rodio::Decoder::try_from(File::open(path)?)?;
    if probe.next().is_none() {
        return Err("no sound decoded (format recognised, but content unreadable)".into());
    }

    // Opens the default audio output (CoreAudio on Mac, ALSA on Linux).
    let mut sink = rodio::DeviceSinkBuilder::open_default_sink()?;
    sink.log_on_drop(false); // avoids a stray message on close

    let player = rodio::Player::connect_new(sink.mixer());
    let file = File::open(path)?;
    player.append(rodio::Decoder::try_from(file)?);

    println!("▶ Playing {}", path.display());
    player.sleep_until_end();
    // `sleep_until_end` returns once rodio has handed over every sample, not
    // once they have left the speakers: some are still in the sound card's
    // buffer. Closing the output right away would drop them, and a very
    // short sample (a 75 ms kick) would vanish entirely.
    thread::sleep(Duration::from_millis(300));
    Ok(())
}
