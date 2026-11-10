// The search box's language, shared so that it can be tested without the
// interface: words found in the path or a tag, `#tag`, and the filters
// `bpm:`, `key:`, `loop:` and `type:`.

use std::error::Error;
use std::ops::RangeInclusive;

use rusqlite::Connection;

use crate::{SampleInfo, analysis, db, sound_type};

/// What a search asks for. A filter still being typed and not valid yet
/// (`bpm:1`, `key:`, `loop:`) filters nothing.
#[derive(Debug, Default, PartialEq)]
pub struct Query {
    /// Tags the sample must carry exactly (`#kick`).
    pub tags: Vec<String>,
    /// Words to find in the path or in one of the tags.
    pub words: Vec<String>,
    pub tempos: Option<RangeInclusive<u32>>,
    /// In sharps, whatever the spelling typed (`key:bbm` gives "A#m").
    pub key: Option<String>,
    pub looped: Option<bool>,
    /// A family (`drums`), a subtype (`kick`) or both (`guitar/electric`).
    pub sound_type: Option<String>,
}

/// Reads the text of the search box.
pub fn parse(text: &str) -> Query {
    let mut query = Query::default();
    for word in text.split_whitespace() {
        let lower = word.to_lowercase();
        if let Some(value) = lower.strip_prefix("bpm:") {
            query.tempos = tempo_range(value).or(query.tempos);
            continue;
        }
        if let Some(value) = word.get(4..).filter(|_| lower.starts_with("key:")) {
            query.key = analysis::normalize_key(value).or(query.key);
            continue;
        }
        if let Some(value) = lower.strip_prefix("loop:") {
            query.looped = match value {
                "yes" | "y" | "true" | "1" => Some(true),
                "no" | "n" | "false" | "0" => Some(false),
                _ => query.looped,
            };
            continue;
        }
        if let Some(value) = lower.strip_prefix("type:") {
            if sound_type::is_known(value) {
                query.sound_type = Some(value.to_string());
            }
            continue;
        }
        match word.strip_prefix('#') {
            Some("") => {} // a lone `#` while the user is still typing
            Some(tag) => query.tags.push(tag.to_string()),
            None => query.words.push(word.to_string()),
        }
    }
    query
}

/// `120` or `118-124`, within the tempos samples have.
fn tempo_range(value: &str) -> Option<RangeInclusive<u32>> {
    let (low, high) = value.split_once('-').unwrap_or((value, value));
    let (low, high): (u32, u32) = (low.parse().ok()?, high.parse().ok()?);
    // In either order: `bpm:150-120` is 120 to 150.
    let (low, high) = (low.min(high), low.max(high));
    // A number still being typed (`bpm:1` on the way to `bpm:120`) filters
    // nothing rather than everything.
    let tempos = analysis::MANUAL_TEMPOS;
    (tempos.contains(&low) && tempos.contains(&high)).then_some(low..=high)
}

impl Query {
    /// Whether the sample passes the filters the database cannot apply:
    /// tempo, key, loop and type are worked out at each search, not stored.
    pub fn keeps(&self, sample: &SampleInfo) -> bool {
        let tempo = self
            .tempos
            .as_ref()
            .is_none_or(|tempos| sample.bpm.is_some_and(|bpm| tempos.contains(&bpm)));
        let key = self.key.is_none() || sample.key == self.key;
        let looped = self.looped.is_none_or(|looped| sample.looped == looped);
        // `type:drums` keeps the whole family, `type:kick` the kicks only,
        // `type:electric` electric guitars and basses.
        let kind = self.sound_type.as_deref().is_none_or(|wanted| {
            sample
                .sound_type
                .as_deref()
                .is_some_and(|found| sound_type::matches(found, wanted))
        });
        tempo && key && looped && kind
    }
}

/// The samples matching the text of the search box. `favorites_only`: only
/// the favorites. `collection`: only the samples of that collection.
pub fn search(
    db: &Connection,
    text: &str,
    favorites_only: bool,
    collection: Option<i64>,
) -> Result<Vec<SampleInfo>, Box<dyn Error>> {
    let query = parse(text);
    let words: Vec<&str> = query.words.iter().map(String::as_str).collect();
    let mut samples = db::search(db, &query.tags, &words, favorites_only, collection)?;
    samples.retain(|sample| query.keeps(sample));
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{memory_db, save_sample};

    fn names(samples: Vec<SampleInfo>) -> Vec<String> {
        samples
            .into_iter()
            .map(|sample| {
                sample
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    /// A small library: the tempo, key, loop and type come from the names.
    fn library() -> Connection {
        let db = memory_db();
        save_sample(&db, "/lib/Loops/Djembe 100BPM D#m.wav");
        save_sample(&db, "/lib/Loops/Bass 120BPM Am.wav");
        let kick = save_sample(&db, "/lib/Drums/Kick_01.wav");
        db::add_tags(&db, kick, &["dry".to_string()]).unwrap();
        db
    }

    fn found(text: &str) -> Vec<String> {
        names(search(&library(), text, false, None).unwrap())
    }

    #[test]
    fn words_and_tags() {
        let query = parse("kick  #Dry #");
        assert_eq!(query.words, ["kick"]);
        assert_eq!(query.tags, ["Dry"]);
        assert_eq!(found("kick"), ["Kick_01.wav"]);
        assert_eq!(found("#DRY"), ["Kick_01.wav"]);
        // A word is found in a tag too ("dry" is in no path), a `#tag` only
        // as a whole tag.
        assert_eq!(found("dry"), ["Kick_01.wav"]);
        assert!(found("#dr").is_empty());
    }

    #[test]
    fn tempo_ranges() {
        assert_eq!(tempo_range("120"), Some(120..=120));
        assert_eq!(tempo_range("118-124"), Some(118..=124));
        assert_eq!(tempo_range("150-120"), Some(120..=150));
        // Any tempo that can be set by hand, 20 to 400.
        assert_eq!(tempo_range("20"), Some(20..=20));
        assert_eq!(tempo_range("400"), Some(400..=400));
        assert_eq!(tempo_range("401"), None);
        // Still being typed: no filter.
        assert_eq!(tempo_range("1"), None);
        assert_eq!(tempo_range("118-"), None);
        assert_eq!(tempo_range("12-130"), None);
    }

    #[test]
    fn tempo_filter() {
        assert_eq!(parse("BPM:120").tempos, Some(120..=120));
        assert_eq!(found("bpm:95-105"), ["Djembe 100BPM D#m.wav"]);
        // A sample without a tempo is left out.
        assert_eq!(found("bpm:20-400").len(), 2);
        // The next filter, still being typed, keeps the last valid one.
        assert_eq!(parse("bpm:120 bpm:1").tempos, Some(120..=120));
        assert_eq!(found("bpm:1").len(), 3);
    }

    #[test]
    fn key_filter() {
        assert_eq!(parse("key:bbm").key.as_deref(), Some("A#m"));
        assert_eq!(parse("KEY:Am").key.as_deref(), Some("Am"));
        assert_eq!(parse("key:").key, None);
        assert_eq!(found("key:am"), ["Bass 120BPM Am.wav"]);
        assert_eq!(found("key:Ebm"), ["Djembe 100BPM D#m.wav"]);
    }

    #[test]
    fn loop_filter() {
        assert_eq!(parse("loop:yes").looped, Some(true));
        assert_eq!(parse("loop:N").looped, Some(false));
        assert_eq!(parse("loop:ye").looped, None);
        assert_eq!(found("loop:no"), ["Kick_01.wav"]);
        assert_eq!(found("loop:yes").len(), 2);
    }

    #[test]
    fn type_filter() {
        assert_eq!(parse("type:Kick").sound_type.as_deref(), Some("kick"));
        assert_eq!(parse("type:nothing").sound_type, None);
        assert_eq!(found("type:kick"), ["Kick_01.wav"]);
        assert_eq!(found("type:drums"), ["Kick_01.wav"]);
        assert_eq!(found("type:bass"), ["Bass 120BPM Am.wav"]);
    }

    #[test]
    fn filters_combine() {
        assert_eq!(found("loop:yes bpm:120 bass"), ["Bass 120BPM Am.wav"]);
        assert!(found("loop:yes type:kick").is_empty());
        // Nothing typed: the whole library, by path.
        assert_eq!(
            found(""),
            ["Kick_01.wav", "Bass 120BPM Am.wav", "Djembe 100BPM D#m.wav"]
        );
    }
}
