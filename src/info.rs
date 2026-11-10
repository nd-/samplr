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
}
