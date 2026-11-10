// The `samplr` command line run as a user would, on generated WAV files and
// a throwaway database (`SAMPLR_DB`): the real one is never touched.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

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
    let frames = 4410u32;
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
        let sample = ((frame % 100) * 50) as i16;
        bytes.extend(sample.to_le_bytes());
        bytes.extend(sample.to_le_bytes());
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
