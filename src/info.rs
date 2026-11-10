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

use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::Hint;

/// The file's metadata as (label, value) pairs, in the file's order, empty
/// values and duplicates left out.
pub fn file_tags(path: &Path) -> Vec<(String, String)> {
    let is_wav = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    let mut tags = if is_wav {
        File::open(path)
            .ok()
            .map(|mut file| riff_info(&mut file))
            .unwrap_or_default()
    } else {
        symphonia_tags(path)
    };
    let mut seen = Vec::new();
    tags.retain(|(label, value)| {
        let keep = !value.is_empty() && !seen.contains(&(label.clone(), value.clone()));
        seen.push((label.clone(), value.clone()));
        keep
    });
    tags
}

/// The `LIST/INFO` chunk of a RIFF file: four-letter fields of text.
fn riff_info<R: Read + Seek>(file: &mut R) -> Vec<(String, String)> {
    let mut header = [0u8; 12];
    if file.read_exact(&mut header).is_err() || &header[0..4] != b"RIFF" {
        return Vec::new();
    }
    let mut tags = Vec::new();
    let mut chunk = [0u8; 8];
    while file.read_exact(&mut chunk).is_ok() {
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as usize;
        // A LIST of a sensible size: its fields, at most a few kilobytes.
        if &chunk[0..4] == b"LIST" && size <= 1 << 20 {
            let mut body = vec![0u8; size];
            if file.read_exact(&mut body).is_err() {
                break;
            }
            if body.starts_with(b"INFO") {
                tags.extend(info_fields(&body[4..]));
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
    tags
}

/// The fields of an INFO list: id (4 bytes), size, text ended by a zero.
fn info_fields(mut body: &[u8]) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    while body.len() >= 8 {
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
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let Ok(mut probed) = symphonia::default::get_probe().format(
        &hint,
        source,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    ) else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    let mut take = |revision: &MetadataRevision| {
        for tag in revision.tags() {
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
    fn not_riff() {
        assert!(riff_info(&mut Cursor::new(b"FORM\0\0\0\0AIFF".to_vec())).is_empty());
    }
}
