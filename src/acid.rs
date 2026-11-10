// The ACID chunk of a WAV file ("Acidized" loops): written by Sony ACID and
// many sample-pack tools, it tells whether the sample is a loop or a
// one-shot, its root note, and its length in beats.
//
// Measured on the 466 files of the Linux library that carry one (see
// CLAUDE.md): its own "tempo" field is 120 in 461 of them, whatever the
// sample, and is not used; the tempo is worked out from the number of beats
// and the length instead, as ACID itself does, and that gave the tempo
// written in the name every one of the 348 times the name has one. The root
// note matched the key of the name 348 times out of 348.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// What the ACID chunk says.
#[derive(Debug, Clone, PartialEq)]
pub struct Acid {
    /// The sample's length in beats; 0 when not set.
    pub beats: u32,
    /// Root note, in sharps ("G#"); the chunk does not say major or minor.
    pub root: Option<String>,
    /// `true` for a loop, `false` for a one-shot.
    pub looped: bool,
}

const NOTES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Flags of the chunk's first field.
const ONE_SHOT: u32 = 0x01;
const ROOT_NOTE_SET: u32 = 0x02;

/// The ACID chunk of a WAV file, if it has one. Only the chunk headers are
/// read (each chunk is skipped with a seek), never the sound. `None` for any
/// other format and for a file that cannot be read.
pub fn read(path: &Path) -> Option<Acid> {
    let is_wav = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    if !is_wav {
        return None;
    }
    let mut file = File::open(path).ok()?;
    find(&mut file)
}

/// Walks the chunks of a RIFF file and parses the `acid` one.
fn find<R: Read + Seek>(file: &mut R) -> Option<Acid> {
    let mut header = [0u8; 12];
    file.read_exact(&mut header).ok()?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }
    let mut chunk = [0u8; 8];
    while file.read_exact(&mut chunk).is_ok() {
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        if &chunk[0..4] == b"acid" {
            let mut body = [0u8; 24];
            file.read_exact(&mut body).ok()?;
            return Some(parse(&body));
        }
        // Chunks are padded to an even size.
        let skip = i64::from(size) + i64::from(size & 1);
        file.seek(SeekFrom::Current(skip)).ok()?;
    }
    None
}

/// The 24 bytes of the chunk: flags (u32), root note (u16, a MIDI note),
/// two unused fields (u16, f32), beats (u32), meter (2 × u16), tempo (f32).
fn parse(body: &[u8; 24]) -> Acid {
    let flags = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    let root_note = u16::from_le_bytes([body[4], body[5]]);
    let beats = u32::from_le_bytes([body[12], body[13], body[14], body[15]]);
    Acid {
        beats,
        root: (flags & ROOT_NOTE_SET != 0).then(|| NOTES[usize::from(root_note % 12)].to_string()),
        looped: flags & ONE_SHOT == 0,
    }
}

/// The tempo of a loop of `beats` beats lasting `seconds`, rounded, when it
/// is a believable one (20 to 400, like a tempo set by hand).
pub fn tempo(beats: u32, seconds: f64) -> Option<u32> {
    if beats == 0 || seconds <= 0.0 {
        return None;
    }
    let tempo = (f64::from(beats) * 60.0 / seconds).round();
    (20.0..=400.0).contains(&tempo).then_some(tempo as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A WAV header with a `fmt ` chunk, then an `acid` one, then `data`.
    fn wav(flags: u32, root: u16, beats: u32) -> Vec<u8> {
        let mut acid = Vec::new();
        acid.extend(flags.to_le_bytes());
        acid.extend(root.to_le_bytes());
        acid.extend(0u16.to_le_bytes());
        acid.extend(0f32.to_le_bytes());
        acid.extend(beats.to_le_bytes());
        acid.extend(4u16.to_le_bytes());
        acid.extend(4u16.to_le_bytes());
        acid.extend(120f32.to_le_bytes());
        let mut file = b"RIFF\0\0\0\0WAVE".to_vec();
        file.extend(b"fmt ");
        file.extend(16u32.to_le_bytes());
        file.extend([0u8; 16]);
        // An odd-sized chunk, padded: the walk must skip the padding byte.
        file.extend(b"odd ");
        file.extend(3u32.to_le_bytes());
        file.extend([1, 2, 3, 0]);
        file.extend(b"acid");
        file.extend(24u32.to_le_bytes());
        file.extend(acid);
        file.extend(b"data");
        file.extend(0u32.to_le_bytes());
        file
    }

    #[test]
    fn loop_with_root_note() {
        let acid = find(&mut Cursor::new(wav(ROOT_NOTE_SET, 68, 32))).unwrap();
        assert_eq!(
            acid,
            Acid {
                beats: 32,
                root: Some("G#".to_string()),
                looped: true
            }
        );
    }

    #[test]
    fn one_shot_without_root_note() {
        let acid = find(&mut Cursor::new(wav(ONE_SHOT, 60, 0))).unwrap();
        assert_eq!(acid.root, None);
        assert!(!acid.looped);
    }

    #[test]
    fn no_chunk() {
        let mut file = b"RIFF\0\0\0\0WAVE".to_vec();
        file.extend(b"data");
        file.extend(0u32.to_le_bytes());
        assert_eq!(find(&mut Cursor::new(file)), None);
        assert_eq!(find(&mut Cursor::new(b"FORM\0\0\0\0AIFF".to_vec())), None);
    }

    #[test]
    fn tempo_from_beats() {
        // 32 beats of "DHO_102_G#m_Good_And_Decent_Dub": 18.82 s.
        assert_eq!(tempo(32, 18.8235), Some(102));
        assert_eq!(tempo(0, 4.0), None);
        assert_eq!(tempo(4, 0.0), None);
        assert_eq!(tempo(1000, 1.0), None);
    }
}
