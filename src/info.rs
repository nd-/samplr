// What a file says about itself, for the Info panel: the `LIST/INFO` chunk
// of a WAV (title, pack, publisher, genre…), which symphonia does not read,
// and the tags symphonia does read in the other formats (ID3 of an MP3,
// Vorbis comments of a FLAC or an OGG…). Read when a sample is selected,
// not stored: nothing searches them yet.
//
// On the Linux library (see CLAUDE.md), 2092 of the 3198 WAV files have a
// `LIST/INFO` chunk (title 1939, pack 1379, publisher 1379, genre 1379…),
// while symphonia found tags in 18 files only, all music MP3 and FLAC.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::meta::{MetadataRevision, StandardTagKey};

/// The file's metadata as (label, value) pairs, in the file's order, empty
/// values and duplicates left out.
pub fn file_tags(path: &Path) -> Vec<(String, String)> {
    let mut tags = if crate::is_wav(path) {
        File::open(path)
            .ok()
            .map(|mut file| riff_info(&mut file))
            .unwrap_or_default()
    } else {
        symphonia_tags(path)
    };
    let mut seen = std::collections::HashSet::new();
    tags.retain(|(label, value)| !value.is_empty() && seen.insert((label.clone(), value.clone())));
    tags.truncate(MAX_FIELDS);
    tags
}

/// At most this many fields are read from a file: a real one has a dozen,
/// and a damaged or crafted file could hold hundreds of thousands.
const MAX_FIELDS: usize = 256;

/// The `LIST/INFO` chunk of a RIFF file: four-letter fields of text.
fn riff_info<R: Read + Seek>(file: &mut R) -> Vec<(String, String)> {
    let mut header = [0u8; 12];
    if file.read_exact(&mut header).is_err() || &header[0..4] != b"RIFF" {
        return Vec::new();
    }
    let mut chunk = [0u8; 8];
    while file.read_exact(&mut chunk).is_ok() {
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as usize;
        // A LIST of a sensible size: its fields, at most a few kilobytes.
        if &chunk[0..4] == b"LIST" && size <= 1 << 20 {
            let mut body = vec![0u8; size];
            if file.read_exact(&mut body).is_err() {
                break;
            }
            // The first INFO list only: a file has one.
            if body.starts_with(b"INFO") {
                return info_fields(&body[4..]);
            }
            if size % 2 == 1 && file.seek(SeekFrom::Current(1)).is_err() {
                break;
            }
            continue;
        }
        let skip = size as i64 + (size as i64 & 1);
        if file.seek(SeekFrom::Current(skip)).is_err() {
            break;
        }
    }
    Vec::new()
}

/// The fields of an INFO list: id (4 bytes), size, text ended by a zero.
fn info_fields(mut body: &[u8]) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    while body.len() >= 8 && fields.len() < MAX_FIELDS {
        let id = String::from_utf8_lossy(&body[0..4]).to_string();
        let size = u32::from_le_bytes([body[4], body[5], body[6], body[7]]) as usize;
        let end = (8 + size).min(body.len());
        let raw = &body[8..end];
        let text = raw.split(|&b| b == 0).next().unwrap_or(&[]);
        let value = decode(text).trim().to_string();
        fields.push((info_label(&id), value));
        body = &body[(end + (size & 1)).min(body.len())..];
    }
    fields
}

/// UTF-8 when it is, otherwise Latin-1, which older tools write.
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// The usual INFO fields by name; another one keeps its four letters.
fn info_label(id: &str) -> String {
    let label = match id {
        "INAM" => "Title",
        "IPRD" => "Album",
        "IART" => "Artist",
        "IAAR" => "Album artist",
        "IGNR" => "Genre",
        "ICMT" => "Comment",
        "ICOP" => "Copyright",
        "ICRD" => "Date",
        "ICMS" => "Commissioned by",
        "IENG" => "Engineer",
        "ISFT" => "Software",
        "ISRC" => "Source",
        "IKEY" => "Keywords",
        "ISBJ" => "Subject",
        "ITCH" => "Technician",
        _ => id,
    };
    label.to_string()
}

/// The tags symphonia reads (ID3, Vorbis comments, MP4…), with readable
/// names for the usual ones.
fn symphonia_tags(path: &Path) -> Vec<(String, String)> {
    let Ok(mut probed) = crate::probe(path) else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    let mut take = |revision: &MetadataRevision| {
        for tag in revision.tags().iter().take(MAX_FIELDS) {
            let label = tag
                .std_key
                .map(standard_label)
                .unwrap_or_else(|| tag.key.clone());
            tags.push((label, tag.value.to_string().trim().to_string()));
        }
    };
    if let Some(metadata) = probed.metadata.get()
        && let Some(revision) = metadata.current()
    {
        take(revision);
    }
    if let Some(revision) = probed.format.metadata().current() {
        take(revision);
    }
    tags
}

fn standard_label(key: StandardTagKey) -> String {
    let label = match key {
        StandardTagKey::TrackTitle => "Title",
        StandardTagKey::Album => "Album",
        StandardTagKey::Artist => "Artist",
        StandardTagKey::AlbumArtist => "Album artist",
        StandardTagKey::Genre => "Genre",
        StandardTagKey::Comment => "Comment",
        StandardTagKey::Copyright => "Copyright",
        StandardTagKey::Date => "Date",
        StandardTagKey::Bpm => "Tempo",
        StandardTagKey::Composer => "Composer",
        StandardTagKey::Label => "Label",
        StandardTagKey::TrackNumber => "Track",
        StandardTagKey::Encoder => "Encoder",
        StandardTagKey::EncodedBy => "Encoded by",
        other => return format!("{other:?}"),
    };
    label.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, encode, write_wav};
    use std::io::Cursor;

    fn field(id: &[u8; 4], text: &[u8]) -> Vec<u8> {
        let mut bytes = id.to_vec();
        bytes.extend((text.len() as u32).to_le_bytes());
        bytes.extend(text);
        if text.len() % 2 == 1 {
            bytes.push(0);
        }
        bytes
    }

    #[test]
    fn info_chunk() {
        let mut list = b"INFO".to_vec();
        list.extend(field(b"IART", b"Splice Originals Hutch Drums\0"));
        list.extend(field(b"IGNR", b"Dub\0"));
        // Latin-1, as older tools write it.
        list.extend(field(b"ICMT", b"Caf\xe9\0"));
        list.extend(field(b"XYZW", b"other\0"));
        let mut file = b"RIFF\0\0\0\0WAVE".to_vec();
        file.extend(b"fmt ");
        file.extend(16u32.to_le_bytes());
        file.extend([0u8; 16]);
        file.extend(b"LIST");
        file.extend((list.len() as u32).to_le_bytes());
        file.extend(list);
        file.extend(b"data");
        file.extend(0u32.to_le_bytes());
        assert_eq!(
            riff_info(&mut Cursor::new(file)),
            vec![
                (
                    "Artist".to_string(),
                    "Splice Originals Hutch Drums".to_string()
                ),
                ("Genre".to_string(), "Dub".to_string()),
                ("Comment".to_string(), "Café".to_string()),
                ("XYZW".to_string(), "other".to_string()),
            ]
        );
    }

    #[test]
    fn capped() {
        // An INFO list of 100 000 empty-ish fields, as a crafted file could
        // hold: read quickly, at most MAX_FIELDS kept.
        let mut list = b"INFO".to_vec();
        for i in 0..100_000u32 {
            list.extend(field(b"ICMT", format!("{i}\0").as_bytes()));
        }
        let fields = info_fields(&list[4..]);
        assert_eq!(fields.len(), MAX_FIELDS);
    }

    #[test]
    fn not_riff() {
        assert!(riff_info(&mut Cursor::new(b"FORM\0\0\0\0AIFF".to_vec())).is_empty());
    }

    /// The file's own tags, with ffmpeg writing title, artist and genre in
    /// the format's own way; `None` when ffmpeg is not installed.
    fn tags_of(name: &str, codec: &[&str]) -> Option<Vec<(String, String)>> {
        let dir = TempDir::new();
        let source = dir.path().join("source.wav");
        write_wav(&source);
        let target = dir.path().join(name);
        let mut args = vec![
            "-metadata",
            "title=Kick 01",
            "-metadata",
            "artist=Hutch",
            "-metadata",
            "genre=Dub",
        ];
        args.extend(codec);
        encode(&source, &target, &args).then(|| file_tags(&target))
    }

    fn has(tags: &[(String, String)], label: &str, value: &str) -> bool {
        tags.iter().any(|(l, v)| l == label && v == value)
    }

    #[test]
    fn tags_of_flac_mp3_and_ogg() {
        for (name, codec) in [
            ("a.flac", &["-c:a", "flac"][..]),
            ("a.mp3", &["-c:a", "libmp3lame"]),
            ("a.ogg", &["-c:a", "libvorbis"]),
        ] {
            let Some(tags) = tags_of(name, codec) else {
                return;
            };
            // ID3 for the MP3, Vorbis comments for the others.
            assert!(has(&tags, "Title", "Kick 01"), "{name}: {tags:?}");
            assert!(has(&tags, "Artist", "Hutch"), "{name}: {tags:?}");
            assert!(has(&tags, "Genre", "Dub"), "{name}: {tags:?}");
        }
    }

    #[test]
    fn tags_of_a_wav_written_by_ffmpeg() {
        // ffmpeg writes them in a LIST/INFO chunk, read by samplr itself.
        let Some(tags) = tags_of("a.wav", &["-c:a", "pcm_s16le"]) else {
            return;
        };
        assert!(has(&tags, "Title", "Kick 01"), "{tags:?}");
        assert!(has(&tags, "Artist", "Hutch"), "{tags:?}");
    }

    #[test]
    fn no_tags_read_from_an_aiff() {
        // ffmpeg writes the title in both an `ID3 ` and a `NAME` chunk;
        // symphonia 0.5 reads neither (see CLAUDE.md). When it does, this
        // test says so.
        let Some(tags) = tags_of("a.aiff", &["-c:a", "pcm_s16be", "-write_id3v2", "1"]) else {
            return;
        };
        assert!(tags.is_empty(), "{tags:?}");
    }

    /// A RIFF/WAVE file of the given chunks, each with `size` as its
    /// declared size whatever its body, padded to an even length.
    fn riff(list: &[(&[u8; 4], u32, Vec<u8>)]) -> Vec<u8> {
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

    /// A LIST/INFO chunk's body of the given fields.
    fn info_list(fields: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut list = b"INFO".to_vec();
        for (id, text) in fields {
            list.extend(field(id, text));
        }
        list
    }

    fn pair(label: &str, value: &str) -> (String, String) {
        (label.to_string(), value.to_string())
    }

    #[test]
    fn found_after_other_lists_and_odd_chunks() {
        let adtl = b"adtlx".to_vec();
        let info = info_list(&[(b"INAM", b"Snare\0")]);
        let file = riff(&[
            (b"fmt ", 16, vec![0; 16]),
            (b"junk", 3, vec![1, 2, 3]),
            (b"LIST", adtl.len() as u32, adtl),
            (b"data", 4, vec![0; 4]),
            (b"LIST", info.len() as u32, info),
        ]);
        assert_eq!(riff_info(&mut Cursor::new(file)), [pair("Title", "Snare")]);
    }

    #[test]
    fn the_first_info_list_only() {
        let first = info_list(&[(b"INAM", b"First\0")]);
        let second = info_list(&[(b"INAM", b"Second\0")]);
        let file = riff(&[
            (b"LIST", first.len() as u32, first),
            (b"LIST", second.len() as u32, second),
        ]);
        assert_eq!(riff_info(&mut Cursor::new(file)), [pair("Title", "First")]);
    }

    #[test]
    fn absurd_list_sizes_give_nothing_without_reading_them() {
        let info = info_list(&[(b"INAM", b"Kick\0")]);
        // Beyond a megabyte, or beyond the file: nothing, and no allocation
        // of the size declared.
        for size in [u32::MAX, (1 << 20) + 1, 1 << 20, info.len() as u32 + 100] {
            let file = riff(&[(b"LIST", size, info.clone())]);
            assert!(riff_info(&mut Cursor::new(file)).is_empty(), "{size}");
        }
        // Shorter than its "INFO" mark.
        let file = riff(&[(b"LIST", 2, b"IN".to_vec())]);
        assert!(riff_info(&mut Cursor::new(file)).is_empty());
    }

    #[test]
    fn fields_with_absurd_sizes() {
        // A field claiming more than the list holds keeps what is there,
        // and ends the list.
        let mut body = b"INAMxxxxKick\0\0".to_vec();
        body[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(info_fields(&body), [pair("Title", "Kick")]);
        // A field of size 0, then another one.
        let mut body = field(b"INAM", b"");
        body.extend(field(b"IGNR", b"Dub\0"));
        assert_eq!(
            info_fields(&body),
            [pair("Title", ""), pair("Genre", "Dub")]
        );
        // Too short for a field's header.
        assert!(info_fields(b"INAM").is_empty());
        assert!(info_fields(b"").is_empty());
    }

    #[test]
    fn text_without_its_zero_is_trimmed_and_utf8_is_kept() {
        let list = info_list(&[
            (b"INAM", b"  Kick \xc3\xa9t\xc3\xa9 \xf0\x9f\xa5\x81  "),
            (b"ICMT", b"before\0after"),
            (b"IGNR", b"\0"),
        ]);
        assert_eq!(
            info_fields(&list[4..]),
            [
                pair("Title", "Kick été 🥁"),
                pair("Comment", "before"),
                pair("Genre", "")
            ]
        );
    }

    #[test]
    fn any_bytes_decode_without_panic() {
        // Every byte, as an id and as text: Latin-1 decodes all of them.
        let all: Vec<u8> = (0..=255).collect();
        let mut body = Vec::new();
        for id in all.chunks(4) {
            body.extend(field(id.try_into().unwrap(), &all));
        }
        assert_eq!(info_fields(&body).len(), 64);
    }

    #[test]
    fn no_prefix_of_a_list_makes_it_panic() {
        let info = info_list(&[
            (b"INAM", b"Kick 01\0"),
            (b"IART", b"Hutch\0"),
            (b"ICMT", b"odd\0"),
        ]);
        let file = riff(&[
            (b"fmt ", 16, vec![0; 16]),
            (b"LIST", info.len() as u32, info),
        ]);
        for length in 0..=file.len() {
            let fields = riff_info(&mut Cursor::new(&file[..length]));
            // Whole, or nothing: a list cut short is not read.
            assert!(fields.is_empty() || length == file.len(), "{length}");
        }
    }

    #[test]
    fn empty_values_and_duplicates_are_left_out() {
        let dir = TempDir::new();
        let path = dir.path().join("dup.wav");
        let info = info_list(&[
            (b"INAM", b"Kick\0"),
            (b"IGNR", b"   \0"),
            (b"INAM", b"Kick\0"),
            (b"IART", b"Kick\0"),
        ]);
        std::fs::write(&path, riff(&[(b"LIST", info.len() as u32, info)])).unwrap();
        assert_eq!(
            file_tags(&path),
            [pair("Title", "Kick"), pair("Artist", "Kick")]
        );
    }

    #[test]
    fn a_crafted_file_keeps_at_most_max_fields() {
        let dir = TempDir::new();
        let path = dir.path().join("many.wav");
        let mut info = b"INFO".to_vec();
        for i in 0..10_000u32 {
            info.extend(field(b"ICMT", format!("{i}\0").as_bytes()));
        }
        std::fs::write(&path, riff(&[(b"LIST", info.len() as u32, info)])).unwrap();
        assert_eq!(file_tags(&path).len(), MAX_FIELDS);
    }

    #[test]
    fn files_that_say_nothing() {
        let dir = TempDir::new();
        for (name, bytes) in [
            ("empty.wav", &b""[..]),
            ("text.wav", b"not a sound"),
            ("empty.flac", b""),
            ("text.mp3", b"ID3"),
            ("text.ogg", b"OggS"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            assert!(file_tags(&path).is_empty(), "{name}");
        }
        // A WAV without tags.
        let path = dir.path().join("plain.wav");
        write_wav(&path);
        assert!(file_tags(&path).is_empty());
    }
}
