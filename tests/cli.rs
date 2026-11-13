// The `samplr` command line run as a user would, on generated WAV files and
// a throwaway database (`SAMPLR_DB`): the real one is never touched.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

/// A folder of its own in the system's temporary folder, deleted with
/// everything inside when the test ends, even when it fails.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "samplr-cli-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        // On the Mac, the temporary folder is behind a link: samplr saves
        // canonical paths, the tests compare with the same.
        TempDir(fs::canonicalize(path).unwrap())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A library to test against: a folder of samples and a database of its own.
struct Library {
    dir: TempDir,
}

impl Library {
    fn new() -> Library {
        let library = Library {
            dir: TempDir::new(),
        };
        fs::create_dir(library.samples()).unwrap();
        library
    }

    fn samples(&self) -> PathBuf {
        self.dir.0.join("samples")
    }

    /// Runs `samplr` with `args` on this library's database.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_samplr"))
            .args(args)
            .env("SAMPLR_DB", self.dir.0.join("samplr.db"))
            .output()
            .unwrap()
    }

    /// Runs `samplr --json` with `args`, which must succeed, and reads its
    /// answer.
    fn json(&self, args: &[&str]) -> Value {
        let mut all = vec!["--json"];
        all.extend(args);
        let output = self.run(&all);
        assert!(
            output.status.success(),
            "samplr {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Writes a sample at `name` below the samples folder; returns its path.
    fn add(&self, name: &str) -> PathBuf {
        let path = self.samples().join(name);
        write_wav(&path);
        path
    }

    fn scan(&self) -> Value {
        self.json(&["scan", self.samples().to_str().unwrap()])
    }
}

/// Writes a WAV (16 bit, stereo, 44.1 kHz, 0.1 s of a quiet ramp) at `path`,
/// creating its folders.
fn write_wav(path: &Path) {
    write_frames(path, 4410, |frame| ((frame % 100) * 50) as i16);
}

/// Half a second of a tone at `hz`.
fn write_tone(path: &Path, hz: f64) {
    write_frames(path, 22_050, |frame| {
        (8000.0 * (2.0 * std::f64::consts::PI * hz * f64::from(frame) / 44_100.0).sin()) as i16
    });
}

/// Half a second of white noise.
fn write_noise(path: &Path) {
    let mut state: u32 = 12345;
    write_frames(path, 22_050, |_| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 16) as i16 / 4
    });
}

/// Writes `frames` of the same sample on both channels at `path`.
fn write_frames(path: &Path, frames: u32, mut sample: impl FnMut(u32) -> i16) {
    let data = frames * 4;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes()); // PCM
    bytes.extend(2u16.to_le_bytes()); // stereo
    bytes.extend(44_100u32.to_le_bytes());
    bytes.extend((44_100u32 * 4).to_le_bytes());
    bytes.extend(4u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    for frame in 0..frames {
        let value = sample(frame);
        bytes.extend(value.to_le_bytes());
        bytes.extend(value.to_le_bytes());
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The file names of a list of samples, in order.
fn names(samples: &Value) -> Vec<String> {
    samples
        .as_array()
        .unwrap()
        .iter()
        .map(|sample| {
            let path = sample["path"].as_str().unwrap();
            Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn scan_then_list() {
    let library = Library::new();
    library.add("kick.wav");
    library.add("drums/snare.wav");
    fs::write(library.samples().join("broken.wav"), "not a sound").unwrap();
    // Left by macOS next to a file, and a disk's trash: never samples.
    library.add("._kick.wav");
    library.add(".Trash-1000/files/old.wav");

    let scan = library.scan();
    assert_eq!(scan["folder"], text(&library.samples()));
    assert_eq!(scan["unreadable"], 1);
    assert_eq!(scan["samples"].as_array().unwrap().len(), 2);

    let list = library.json(&["list"]);
    assert_eq!(names(&list), ["snare.wav", "kick.wav"]);
    let kick = &list[1];
    assert_eq!(kick["path"], text(&library.samples().join("kick.wav")));
    assert_eq!(kick["channels"], 2);
    assert_eq!(kick["bits"], 16);
    assert!((kick["duration_secs"].as_f64().unwrap() - 0.1).abs() < 0.001);
}

#[test]
fn tags_are_added_searched_and_removed() {
    let library = Library::new();
    let kick = library.add("kick.wav");
    library.add("snare.wav");
    library.scan();

    let tagged = library.json(&["tag", text(&kick), "Drums", "dry"]);
    assert_eq!(tagged["added"], 2);
    let found = library.json(&["search", "drums"]);
    assert_eq!(names(&found), ["kick.wav"]);
    assert_eq!(found[0]["tags"], serde_json::json!(["drums", "dry"]));
    assert_eq!(
        library.json(&["tags"]),
        serde_json::json!([{ "name": "drums", "count": 1 }, { "name": "dry", "count": 1 }])
    );

    let untagged = library.json(&["untag", text(&kick), "DRUMS"]);
    assert_eq!(untagged["removed"], 1);
    // No sample carries "drums" any more: the tag is gone.
    assert_eq!(
        library.json(&["tags"]),
        serde_json::json!([{ "name": "dry", "count": 1 }])
    );
}

#[test]
fn search_reads_the_interface_syntax() {
    let library = Library::new();
    library.add("Loops/Bass 120BPM Am.wav");
    library.add("Loops/Pad 90BPM Am.wav");
    let kick = library.add("Drums/Kick_01.wav");
    library.scan();
    library.json(&["tag", text(&kick), "dry"]);

    assert_eq!(names(&library.json(&["search", "#dry"])), ["Kick_01.wav"]);
    assert_eq!(
        names(&library.json(&["search", "loop:no"])),
        ["Kick_01.wav"]
    );
    assert_eq!(
        names(&library.json(&["search", "type:kick"])),
        ["Kick_01.wav"]
    );
    assert_eq!(
        names(&library.json(&["search", "key:am", "bpm:100-130"])),
        ["Bass 120BPM Am.wav"]
    );
    // Words given as one argument or several: the same search.
    assert_eq!(
        names(&library.json(&["search", "loops am"])),
        names(&library.json(&["search", "loops", "am"]))
    );
}

#[test]
fn favorites() {
    let library = Library::new();
    let kick = library.add("kick.wav");
    library.add("snare.wav");
    library.scan();

    library.json(&["fav", text(&kick)]);
    assert_eq!(names(&library.json(&["search", "--fav"])), ["kick.wav"]);
    library.json(&["unfav", text(&kick)]);
    assert!(names(&library.json(&["search", "--fav"])).is_empty());
}

#[test]
fn scanning_again_keeps_the_tags_without_duplicates() {
    let library = Library::new();
    let kick = library.add("kick.wav");
    library.scan();
    library.json(&["tag", text(&kick), "drums"]);

    library.scan();
    let list = library.json(&["list"]);
    assert_eq!(names(&list), ["kick.wav"]);
    assert_eq!(list[0]["tags"], serde_json::json!(["drums"]));
}

#[test]
fn a_removed_folder_comes_back_with_its_tags() {
    let library = Library::new();
    let kick = library.add("kick.wav");
    library.scan();
    library.json(&["tag", text(&kick), "drums"]);
    let folder = text(&library.samples()).to_string();

    let removed = library.json(&["remove-folder", &folder]);
    assert_eq!(removed["hidden"], 1);
    assert_eq!(library.json(&["folders"]), serde_json::json!([]));
    assert!(names(&library.json(&["list"])).is_empty());

    library.scan();
    let folders = library.json(&["folders"]);
    assert_eq!(
        folders,
        serde_json::json!([{ "path": folder, "samples": 1, "found": true }])
    );
    assert_eq!(
        library.json(&["list"])[0]["tags"],
        serde_json::json!(["drums"])
    );
}

#[test]
fn waveforms_are_computed_once() {
    let library = Library::new();
    let kick = library.add("kick.wav");
    library.add("snare.wav");
    library.scan();

    let first = library.json(&["waveforms"]);
    assert_eq!(
        (first["computed"].as_u64(), first["up_to_date"].as_u64()),
        (Some(2), Some(0))
    );
    let again = library.json(&["waveforms"]);
    assert_eq!(
        (again["computed"].as_u64(), again["up_to_date"].as_u64()),
        (Some(0), Some(2))
    );

    let wave = library.json(&["wave", text(&kick)]);
    assert_eq!(wave["peaks"].as_array().unwrap().len(), 1024);
}

#[test]
fn errors_go_to_stderr_with_exit_code_1() {
    let library = Library::new();
    // On disk, but never scanned.
    let outside = library.add("kick.wav");

    for args in [
        vec!["untag", text(&outside), "drums"],
        vec!["unfav", text(&outside)],
    ] {
        let output = library.run(&args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty());
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(message.contains("is not in the database"), "{message}");
    }

    let output = library.run(&["remove-folder", "/nowhere/samplr-test"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not a scanned folder"));
}

#[test]
fn a_closed_pipe_ends_quietly() {
    let library = Library::new();
    // More output than a pipe holds (64 KB on Linux), so that writing goes
    // on after the reader has gone.
    for i in 0..200 {
        library.add(&format!("sample {i:03}.wav"));
    }
    library.scan();

    // `samplr --json list | head -c 10`: read a little, then close.
    let mut child = Command::new(env!("CARGO_BIN_EXE_samplr"))
        .args(["--json", "list"])
        .env("SAMPLR_DB", library.dir.0.join("samplr.db"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut start = [0u8; 10];
    stdout.read_exact(&mut start).unwrap();
    drop(stdout);

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tag_and_fav_add_a_file_not_yet_scanned() {
    let library = Library::new();
    let kick = library.add("kick.wav");
    let snare = library.add("snare.wav");

    library.json(&["tag", text(&kick), "drums"]);
    library.json(&["fav", text(&snare)]);
    let list = library.json(&["list"]);
    assert_eq!(names(&list), ["kick.wav", "snare.wav"]);
    assert_eq!(list[0]["tags"], serde_json::json!(["drums"]));
    assert_eq!(list[1]["favorite"], true);
    // Known to the database, but its folder is not followed.
    assert_eq!(library.json(&["folders"]), serde_json::json!([]));
}

#[test]
fn only_audio_files_are_tagged() {
    let library = Library::new();
    let notes = library.samples().join("notes.txt");
    fs::write(&notes, "not a sound").unwrap();
    let output = library.run(&["tag", text(&notes), "drums"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not a known audio file"));
}

#[test]
fn the_waveform_drawn_in_the_terminal() {
    let library = Library::new();
    let kick = library.add("kick.wav");
    let output = library.run(&["wave", text(&kick), "--width", "20"]);
    assert!(output.status.success());
    let drawing = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = drawing.lines().collect();
    // Six lines of twenty columns, then the file's path.
    assert_eq!(lines.len(), 7);
    assert!(lines[..6].iter().all(|line| line.chars().count() == 20));
    assert_eq!(lines[6], text(&kick));
}

#[test]
fn similar_samples_nearest_first() {
    let library = Library::new();
    let low = library.samples().join("low.wav");
    let samples = [
        (low.clone(), 200.0),
        (library.samples().join("near.wav"), 220.0),
        (library.samples().join("high.wav"), 3000.0),
    ];
    for (path, hz) in &samples {
        write_tone(path, *hz);
    }
    write_noise(&library.samples().join("hiss.wav"));
    write_wav(&library.samples().join("ramp.wav"));
    library.scan();

    // Not measured yet: only the sample asked about is, on the way.
    let answer = library.json(&["similar", text(&low)]);
    assert_eq!(answer["file"], text(&low));
    assert!(answer["similar"].as_array().unwrap().is_empty());

    library.json(&["waveforms"]);
    let answer = library.json(&["similar", text(&low), "-n", "2"]);
    let similar = answer["similar"].as_array().unwrap();
    assert_eq!(names(&answer["similar"]), ["near.wav", "high.wav"]);
    let distances: Vec<f64> = similar
        .iter()
        .map(|sample| sample["distance"].as_f64().unwrap())
        .collect();
    assert!(distances[0] < distances[1]);

    let output = library.run(&["similar", text(&low)]);
    let table = String::from_utf8(output.stdout).unwrap();
    assert!(
        table.lines().nth(2).unwrap().starts_with("DIST."),
        "{table}"
    );
    assert_eq!(table.lines().count(), 3 + 4);

    let outside = library.dir.0.join("outside.wav");
    write_wav(&outside);
    let output = library.run(&["similar", text(&outside)]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not in the database"));
}

#[test]
fn watching_needs_a_scanned_folder() {
    let library = Library::new();
    let output = library.run(&["watch"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("no folder to watch"));
}

/// A `samplr --json watch` running in the background; stopped when dropped.
struct Watch {
    child: std::process::Child,
    lines: mpsc::Receiver<String>,
}

impl Watch {
    fn start(library: &Library) -> Watch {
        let mut child = Command::new(env!("CARGO_BIN_EXE_samplr"))
            .args(["--json", "watch"])
            .env("SAMPLR_DB", library.dir.0.join("samplr.db"))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // Read in a thread of its own, so that a missing line makes the test
        // fail after a while instead of waiting forever.
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Watch { child, lines }
    }

    /// Waits for a change reported as `expected`, skipping the others.
    fn expect(&self, expected: &str) {
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| panic!("no change \"{expected}\" reported"));
            let change: Value = serde_json::from_str(&line).unwrap();
            if change["change"] == expected {
                return;
            }
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn watching_reports_each_change_as_a_line_of_json() {
    let library = Library::new();
    library.add("kick.wav");
    library.scan();
    let root = text(&library.samples()).to_string();

    let watch = Watch::start(&library);
    // The catch-up first, then the changes as they come.
    watch.expect(&format!("{root}: 1 sample(s), 0 removed, 0 unreadable"));
    let snare = library.add("snare.wav");
    watch.expect(&format!("+ {}", text(&snare)));
    fs::remove_file(&snare).unwrap();
    watch.expect(&format!("- {} (1 sample(s))", text(&snare)));
    drop(watch);

    assert_eq!(names(&library.json(&["list"])), ["kick.wav"]);
}

#[test]
fn a_wav_declaring_no_sample_rate_is_unreadable_not_a_crash() {
    let library = Library::new();
    library.add("kick.wav");
    let broken = library.samples().join("zero rate.wav");
    write_wav(&broken);
    let mut bytes = fs::read(&broken).unwrap();
    // The `fmt ` chunk's sample rate.
    bytes[24..28].copy_from_slice(&0u32.to_le_bytes());
    fs::write(&broken, bytes).unwrap();

    // Expected: the scan ends, the damaged file counted as unreadable.
    let scan = library.scan();
    assert_eq!(scan["unreadable"], 1);
    assert_eq!(scan["samples"].as_array().unwrap().len(), 1);
}

#[test]
fn odd_file_names_through_the_command_line() {
    let library = Library::new();
    let names_on_disk = [
        "Été/caisse claire.wav",
        "🥁 kits/boom.wav",
        "鼓/キック.wav",
        "100% wet.wav",
        "a_b.wav",
        "#1 snare.wav",
        "it's \"loud\".wav",
    ];
    for name in names_on_disk {
        library.add(name);
    }
    let scan = library.scan();
    assert_eq!(
        scan["samples"].as_array().unwrap().len(),
        names_on_disk.len()
    );

    let boom = library.samples().join("🥁 kits/boom.wav");
    assert_eq!(library.json(&["tag", text(&boom), "ÉTÉ"])["added"], 1);
    assert_eq!(names(&library.json(&["search", "#été"])), ["boom.wav"]);
    assert_eq!(names(&library.json(&["search", "🥁"])), ["boom.wav"]);
    assert_eq!(names(&library.json(&["search", "キック"])), ["キック.wav"]);
    assert_eq!(names(&library.json(&["search", "%"])), ["100% wet.wav"]);
    assert_eq!(names(&library.json(&["search", "_"])), ["a_b.wav"]);
    assert_eq!(
        names(&library.json(&["search", "'s"])),
        ["it's \"loud\".wav"]
    );
    assert_eq!(
        library.json(&["tags"]),
        serde_json::json!([{ "name": "été", "count": 1 }])
    );

    let removed = library.json(&["remove-folder", text(&library.samples())]);
    assert_eq!(removed["hidden"], names_on_disk.len());
    assert!(names(&library.json(&["list"])).is_empty());
}

#[test]
fn odd_searches_filter_nothing_by_mistake() {
    let library = Library::new();
    library.add("kick.wav");
    library.add("snare.wav");
    library.scan();
    for text in [
        "#",
        "bpm:abc",
        "bpm:5-",
        "key:zzz",
        "type:nothing",
        "loop:maybe",
        "",
    ] {
        assert_eq!(names(&library.json(&["search", text])).len(), 2, "{text:?}");
    }
    assert!(names(&library.json(&["search", "##"])).is_empty());
}

#[test]
fn an_unplugged_folder_keeps_its_samples() {
    let library = Library::new();
    let kick = library.add("kick.wav");
    library.scan();
    library.json(&["tag", text(&kick), "kept"]);
    let folder = text(&library.samples()).to_string();
    let away = library.dir.0.join("unplugged");
    fs::rename(library.samples(), &away).unwrap();

    assert_eq!(
        library.json(&["folders"]),
        serde_json::json!([{ "path": folder, "samples": 1, "found": false }])
    );
    // Nothing forgotten: plugged back, the tags are there.
    fs::rename(&away, library.samples()).unwrap();
    assert_eq!(
        library.json(&["list"])[0]["tags"],
        serde_json::json!(["kept"])
    );
}

#[test]
fn scanning_a_missing_folder_fails_without_saving_it() {
    let library = Library::new();
    let missing = library.dir.0.join("missing");
    let output = library.run(&["scan", text(&missing)]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not a folder"));
    assert_eq!(library.json(&["folders"]), serde_json::json!([]));
}

#[test]
fn reading_while_another_process_writes() {
    let library = Library::new();
    library.add("kick.wav");
    library.scan();

    // The interface or a scan in the middle of a write: an up-to-date
    // database is read without waiting for the write lock.
    let writer = rusqlite::Connection::open(library.dir.0.join("samplr.db")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    // Needing the lock, each would fail with "database is locked".
    assert_eq!(names(&library.json(&["list"])), ["kick.wav"]);
    assert_eq!(names(&library.json(&["search", "kick"])), ["kick.wav"]);
    writer.execute_batch("ROLLBACK").unwrap();
}
