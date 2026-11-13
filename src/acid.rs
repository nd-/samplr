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

use crate::analysis;

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

/// Flags of the chunk's first field.
const ONE_SHOT: u32 = 0x01;
const ROOT_NOTE_SET: u32 = 0x02;

/// The ACID chunk of a WAV file, if it has one. Only the chunk headers are
/// read (each chunk is skipped with a seek), never the sound. `None` for any
/// other format and for a file that cannot be read.
pub fn read(path: &Path) -> Option<Acid> {
    if !crate::is_wav(path) {
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
            // Shorter, its fields would be read from the next chunk.
            if size < 24 {
                return None;
            }
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
        root: (flags & ROOT_NOTE_SET != 0)
            .then(|| analysis::NOTES[usize::from(root_note % 12)].to_string()),
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
    let tempo = tempo as u32;
    analysis::MANUAL_TEMPOS.contains(&tempo).then_some(tempo)
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
        // The tempos a sample can be given by hand, 20 to 400.
        assert_eq!(tempo(20, 60.0), Some(20));
        assert_eq!(tempo(400, 60.0), Some(400));
        assert_eq!(tempo(19, 60.0), None);
        assert_eq!(tempo(401, 60.0), None);
    }

    #[test]
    fn tempo_from_odd_lengths_or_beats() {
        for seconds in [
            -4.0,
            -0.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN_POSITIVE,
            5e-324,
            f64::MAX,
        ] {
            assert_eq!(tempo(8, seconds), None, "{seconds}");
            assert_eq!(tempo(u32::MAX, seconds), None, "{seconds}");
        }
        assert_eq!(tempo(u32::MAX, 1.0), None);
        // Rounded first: 19.5 is taken as 20, 400.49 as 400.
        assert_eq!(tempo(13, 40.0), Some(20));
        assert_eq!(tempo(40_049, 6000.0), Some(400));
    }

    #[test]
    fn tempo_from_beats_stays_in_its_range() {
        for beats in [1, 2, 3, 4, 7, 8, 16, 64, 128, 1000, u32::MAX] {
            for centis in 1..20_000 {
                let seconds = f64::from(centis) / 100.0;
                if let Some(tempo) = tempo(beats, seconds) {
                    assert!(
                        analysis::MANUAL_TEMPOS.contains(&tempo),
                        "{beats} beats in {seconds} s: {tempo}"
                    );
                }
            }
        }
    }

    /// A RIFF/WAVE file of the given chunks, each written with `size` as
    /// its declared size whatever its body, padded to an even length.
    fn chunks(list: &[(&[u8; 4], u32, Vec<u8>)]) -> Vec<u8> {
        let mut file = b"RIFF\0\0\0\0WAVE".to_vec();
        for (id, size, body) in list {
            file.extend(*id);
            file.extend(size.to_le_bytes());
            file.extend(body);
            if body.len() % 2 == 1 {
                file.push(0);
            }
        }
        file
    }

    fn body(flags: u32, root: u16, beats: u32) -> Vec<u8> {
        let mut body = flags.to_le_bytes().to_vec();
        body.extend(root.to_le_bytes());
        body.extend([0u8; 6]);
        body.extend(beats.to_le_bytes());
        body.extend([4, 0, 4, 0]);
        body.extend(120f32.to_le_bytes());
        body
    }

    #[test]
    fn after_the_sound_and_odd_chunks() {
        // A megabyte of sound skipped with a seek, then odd-sized chunks.
        let file = chunks(&[
            (b"fmt ", 16, vec![0; 16]),
            (b"data", 1 << 20, vec![0; 1 << 20]),
            (b"junk", 1, vec![7]),
            (b"bext", 5, vec![1; 5]),
            (b"acid", 24, body(0, 61, 16)),
        ]);
        let acid = find(&mut Cursor::new(file)).unwrap();
        assert_eq!((acid.beats, acid.root, acid.looped), (16, None, true));
    }

    #[test]
    fn any_root_note_is_a_note() {
        for root in [0, 11, 12, 127, 128, u16::MAX] {
            let file = chunks(&[(b"acid", 24, body(ROOT_NOTE_SET, root, 4))]);
            let acid = find(&mut Cursor::new(file)).unwrap();
            assert!(analysis::NOTES.contains(&acid.root.as_deref().unwrap()));
        }
        // MIDI 60 is C, 69 is A.
        let note = |root| find(&mut Cursor::new(chunks(&[(b"acid", 24, body(2, root, 4))])));
        assert_eq!(note(60).unwrap().root.as_deref(), Some("C"));
        assert_eq!(note(69).unwrap().root.as_deref(), Some("A"));
    }

    #[test]
    fn a_chunk_cut_short_is_not_read() {
        // The file ends inside the chunk.
        let mut file = chunks(&[(b"acid", 24, body(0, 60, 8))]);
        file.truncate(file.len() - 4);
        assert_eq!(find(&mut Cursor::new(file)), None);
    }

    #[test]
    fn a_chunk_declared_shorter_than_its_fields_is_not_read() {
        // 8 bytes declared, then a data chunk: its bytes are not the acid
        // chunk's beats. Expected: `None`, the chunk being too short.
        let file = chunks(&[
            (b"acid", 8, body(0, 60, 8)[..8].to_vec()),
            (b"data", 16, vec![0xff; 16]),
        ]);
        assert_eq!(find(&mut Cursor::new(file)), None);
    }

    #[test]
    fn absurd_chunk_sizes_end_the_walk() {
        for size in [u32::MAX, u32::MAX - 1, 1 << 31] {
            let file = chunks(&[(b"junk", size, vec![0; 8]), (b"acid", 24, body(0, 60, 8))]);
            assert_eq!(find(&mut Cursor::new(file)), None, "{size}");
        }
        // Zero-sized chunks are walked through, one by one.
        let mut list: Vec<(&[u8; 4], u32, Vec<u8>)> = vec![(b"junk", 0, Vec::new()); 10_000];
        list.push((b"acid", 24, body(0, 60, 8)));
        assert!(find(&mut Cursor::new(chunks(&list))).is_some());
    }

    #[test]
    fn only_a_riff_wave_is_read() {
        let acid = body(0, 60, 8);
        for header in [&b"RIFX\0\0\0\0WAVE"[..], b"RIFF\0\0\0\0AVI ", b"RIFF", b""] {
            let mut file = header.to_vec();
            file.extend(b"acid");
            file.extend(24u32.to_le_bytes());
            file.extend(&acid);
            assert_eq!(find(&mut Cursor::new(file)), None, "{header:?}");
        }
    }

    #[test]
    fn read_after_the_extension_whatever_its_case() {
        let dir = crate::test_support::TempDir::new();
        let file = chunks(&[(b"acid", 24, body(ROOT_NOTE_SET, 60, 8))]);
        for name in ["a.wav", "B.WAV", "c.Wav", "été 🥁 #1.wav"] {
            std::fs::write(dir.path().join(name), &file).unwrap();
            assert!(read(&dir.path().join(name)).is_some(), "{name}");
        }
        // The same bytes under another extension are not looked at.
        for name in ["a.aiff", "a.wav.txt", "wav"] {
            std::fs::write(dir.path().join(name), &file).unwrap();
            assert_eq!(read(&dir.path().join(name)), None, "{name}");
        }
    }

    #[test]
    fn tempo_of_lengths_that_are_not_numbers() {
        for seconds in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.0,
            f64::MIN_POSITIVE,
        ] {
            assert_eq!(tempo(8, seconds), None, "{seconds}");
        }
        assert_eq!(tempo(u32::MAX, 1.0), None);
        // Rounded to the nearest: 119.6 and 120.4 are 120.
        assert_eq!(tempo(4, 4.0 * 60.0 / 119.6), Some(120));
        assert_eq!(tempo(4, 4.0 * 60.0 / 120.4), Some(120));
    }
}
