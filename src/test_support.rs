// Helpers shared by the tests that need a database or real files.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use rusqlite::Connection;

use crate::db;

/// A database in memory, at the current schema.
pub fn memory_db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db::set_up(&db).unwrap();
    db
}

/// A folder of its own in the system's temporary folder, deleted with
/// everything inside when the test ends, even when it fails.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "samplr-test-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        TempDir(dunce::canonicalize(path).unwrap())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Writes a short WAV (16 bit, mono, 44.1 kHz, 0.1 s of silence) at `path`,
/// creating its folders: a file `read_info` accepts.
pub fn write_wav(path: &Path) {
    write_sound(path, 1, &[0; 4410]);
}

/// Writes a 16-bit, 44.1 kHz WAV of `channels` channels at `path`, creating
/// its folders. `samples` are interleaved: left, right, left…
pub fn write_sound(path: &Path, channels: u16, samples: &[i16]) {
    let data = (samples.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes()); // PCM
    bytes.extend(channels.to_le_bytes());
    bytes.extend(44_100u32.to_le_bytes());
    bytes.extend((44_100 * 2 * u32::from(channels)).to_le_bytes());
    bytes.extend((2 * channels).to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    for sample in samples {
        bytes.extend(sample.to_le_bytes());
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
