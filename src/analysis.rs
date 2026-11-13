// Tempo and key of a sample, from what is already known about it: its file
// name, where sample packs write them ("Djembe 100BPM D#m", "YCE_120_Gm_…"),
// and its length, a whole number of beats for a loop.
//
// Measured on 3287 samples (see CLAUDE.md): a tempo found from the length is
// right 87% of the time, when the length gives a whole tempo; an analysis of
// the sound itself (stratum-dsp) was right 34% of the time for the tempo and
// 13% for the key, and is not used.

use serde::Serialize;

/// Where a value comes from.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Written in the file name.
    Name,
    /// Computed from the sample's length: an estimate.
    Length,
    /// Set by hand in the interface.
    Manual,
    /// From the file's ACID chunk: its length in beats (see `acid`).
    File,
    /// Detected from the sound (see `pulse`): an estimate.
    Sound,
}

impl Source {
    /// The source named as serde writes it ("name", "sound"…).
    pub fn from_name(name: &str) -> Option<Source> {
        match name {
            "name" => Some(Source::Name),
            "length" => Some(Source::Length),
            "manual" => Some(Source::Manual),
            "file" => Some(Source::File),
            "sound" => Some(Source::Sound),
            _ => None,
        }
    }
}

/// The tempos a name may give: sample packs stay within this range.
const TEMPOS: std::ops::RangeInclusive<u32> = 50..=250;

/// The tempos a sample can be given by hand, hence searched for.
pub const MANUAL_TEMPOS: std::ops::RangeInclusive<u32> = 20..=400;

pub(crate) const NOTES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// The file name cut into words: "YCE_120_Gm_Horns (Wet)" gives "YCE",
/// "120", "Gm", "Horns", "Wet".
fn words(stem: &str) -> Vec<&str> {
    stem.split([' ', '_', '-', '(', ')', '[', ']', '.', ','])
        .filter(|word| !word.is_empty())
        .collect()
}

fn tempo_word(word: &str) -> Option<u32> {
    if word.len() < 2 || word.len() > 3 || !word.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let tempo: u32 = word.parse().ok()?;
    TEMPOS.contains(&tempo).then_some(tempo)
}

/// "120bpm", "120BPM", or "120" followed by the word "bpm".
fn written_tempo(words: &[&str]) -> Option<u32> {
    for (i, word) in words.iter().enumerate() {
        let lower = word.to_lowercase();
        if let Some(number) = lower.strip_suffix("bpm")
            && let Some(tempo) = tempo_word(number)
        {
            return Some(tempo);
        }
        if words
            .get(i + 1)
            .is_some_and(|next| next.eq_ignore_ascii_case("bpm"))
            && let Some(tempo) = tempo_word(word)
        {
            return Some(tempo);
        }
    }
    None
}

/// A key written as a word: "C", "F#", "Bb", "Am", "Ebm", "Gmin", "Emaj".
/// Returns the key in sharps ("A#m" for "Bbm") and whether it was written
/// with a quality ("m", "maj"…), which makes a lone letter believable.
/// The note's letter must be upper case: "a", "am" or "em" in a name are
/// more often words than keys.
fn key_word(word: &str) -> Option<(String, bool)> {
    let mut chars = word.chars();
    let letter = chars.next()?;
    if !('A'..='G').contains(&letter) {
        return None;
    }
    // The note: its letter, maybe a sharp or a flat, then its quality.
    let rest: String = chars.collect();
    let (accidental, quality) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') => (-1, &rest[1..]),
        _ => (0, &rest[..]),
    };
    let minor = match quality.to_lowercase().as_str() {
        "" => false,
        "m" | "min" | "minor" => true,
        "maj" | "major" => false,
        _ => return None,
    };
    let natural = NOTES.iter().position(|n| *n == letter.to_string())?;
    let note = NOTES[(natural as i32 + accidental).rem_euclid(12) as usize];
    let marked = !quality.is_empty() || accidental != 0;
    Some((format!("{note}{}", if minor { "m" } else { "" }), marked))
}

/// The tempo written in a file name (without its extension), if any.
pub fn tempo_from_name(stem: &str) -> Option<u32> {
    let words = words(stem);
    if let Some(tempo) = written_tempo(&words) {
        return Some(tempo);
    }
    // A bare number right before a key: "YCE_120_Gm_Horns".
    words.windows(2).find_map(|pair| {
        let tempo = tempo_word(pair[0])?;
        key_word(pair[1]).map(|_| tempo)
    })
}

/// The key written in a file name (without its extension), in sharps ("A#m"
/// rather than "Bbm"), if any. A lone letter counts only next to a tempo,
/// as sample packs write it: "Kick_A" is not in A. A key with its quality
/// wins over a lone letter wherever they stand ("Kit_A_Loop_120_Gm" is in Gm).
pub fn key_from_name(stem: &str) -> Option<String> {
    let keys: Vec<(String, bool)> = words(stem)
        .iter()
        .filter_map(|word| key_word(word))
        .collect();
    let qualified = keys.iter().find(|(_, qualified)| *qualified);
    let lone = keys.first().filter(|_| tempo_from_name(stem).is_some());
    qualified.or(lone).map(|(key, _)| key.clone())
}

/// The tempo of a loop from its length: a loop lasts a whole number of
/// bars, so the tempo is 60 × beats ÷ seconds. Only when that gives a whole
/// tempo (a one-shot almost never does), and of the possible ones, the
/// nearest to 120.
pub fn tempo_from_length(seconds: f64) -> Option<u32> {
    if seconds <= 0.0 {
        return None;
    }
    [4, 8, 12, 16, 24, 32, 48, 64, 96, 128]
        .iter()
        .map(|beats| 60.0 * *beats as f64 / seconds)
        .filter(|tempo| (70.0..=180.0).contains(tempo) && (tempo - tempo.round()).abs() <= 0.02)
        .min_by(|a, b| (a - 120.0).abs().total_cmp(&(b - 120.0).abs()))
        .map(|tempo| tempo.round() as u32)
}

/// The tempo of a sample and where it comes from: its name, else its length.
pub fn tempo(stem: &str, seconds: Option<f64>, acid_beats: Option<u32>) -> Option<(u32, Source)> {
    if let Some(tempo) = tempo_from_name(stem) {
        return Some((tempo, Source::Name));
    }
    // The ACID chunk's beats give the exact tempo; the length alone only a
    // guess.
    if let (Some(beats), Some(seconds)) = (acid_beats, seconds)
        && let Some(tempo) = crate::acid::tempo(beats, seconds)
    {
        return Some((tempo, Source::File));
    }
    seconds
        .and_then(tempo_from_length)
        .map(|tempo| (tempo, Source::Length))
}

/// Loop or one-shot, from the words of the file name, then of its folders
/// from the deepest up: the first that says it wins, so that
/// `Drums Loops/HITS/Kick/kick_01.wav` is a one-shot. "loop", "loops" say
/// loop; "one shot", "oneshot", "hit", "hits", "fx", "sfx", "fill", "fills"
/// say one-shot. `None` when no word says it.
pub fn loop_from_words(path: &std::path::Path) -> Option<bool> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let folders = path
        .parent()
        .into_iter()
        .flat_map(|folder| folder.iter().rev())
        .filter_map(|part| part.to_str());
    for part in std::iter::once(stem).chain(folders) {
        let lower = part.to_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        let one_shot = words.iter().any(|word| {
            // Effects and drum fills are played once too.
            matches!(
                *word,
                "oneshot" | "oneshots" | "hit" | "hits" | "fx" | "sfx" | "fill" | "fills"
            )
        }) || words
            .windows(2)
            .any(|pair| pair[0] == "one" && pair[1].starts_with("shot"));
        if one_shot {
            return Some(false);
        }
        if words.iter().any(|word| matches!(*word, "loop" | "loops")) {
            return Some(true);
        }
    }
    None
}

/// Loop or one-shot, and where it comes from: the ACID chunk's flag, else
/// the words of the path, else a loop when a tempo is known (from the name,
/// the chunk or the length), a one-shot otherwise (an estimate).
pub fn looped(path: &std::path::Path, acid_loop: Option<bool>, has_tempo: bool) -> (bool, Source) {
    if let Some(looped) = acid_loop {
        return (looped, Source::File);
    }
    if let Some(looped) = loop_from_words(path) {
        return (looped, Source::Name);
    }
    (has_tempo, Source::Length)
}

/// The same key whatever the spelling: "Bbm", "A#m", "bbmin" give "A#m".
pub fn normalize_key(key_text: &str) -> Option<String> {
    let mut word = key_text.trim().to_string();
    // A lower-case first letter is fine when someone types it.
    if let Some(first) = word.get(..1) {
        word = first.to_uppercase() + &word[1..];
    }
    key_word(&word).map(|(key, _)| key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_written_with_bpm() {
        assert_eq!(tempo_from_name("1-Djembe-100BPM-D#m-Full"), Some(100));
        assert_eq!(tempo_from_name("Atmos Flute 01 (110BPM)"), Some(110));
        assert_eq!(tempo_from_name("FL_DubDrums_043_lofi_79bpm"), Some(79));
        assert_eq!(tempo_from_name("Loop 128 bpm"), Some(128));
    }

    #[test]
    fn tempo_bare_before_a_key() {
        assert_eq!(tempo_from_name("YCE_120_Gm_Trumpet_Style_Wet_1"), Some(120));
        assert_eq!(tempo_from_name("MP2_133_Bbm_Guitar"), Some(133));
        assert_eq!(tempo_from_name("DHO_72_Dm_Chalwa_Horns_Swell"), Some(72));
    }

    #[test]
    fn no_tempo_from_other_numbers() {
        assert_eq!(tempo_from_name("MP2_Snare_20"), None);
        assert_eq!(tempo_from_name("FL_DubDrums_022"), None);
        assert_eq!(tempo_from_name("Kick 808"), None);
        // A number without a key after it is not taken for a tempo.
        assert_eq!(tempo_from_name("MP2_84_Drums_1"), None);
    }

    #[test]
    fn keys_from_names() {
        assert_eq!(
            key_from_name("1-Djembe-100BPM-D#m-Full").as_deref(),
            Some("D#m")
        );
        assert_eq!(
            key_from_name("4-Djembe-105BPM-Emaj_Kick").as_deref(),
            Some("E")
        );
        assert_eq!(
            key_from_name("YCE_140_D_Elec_Gtr_Beater_Chords").as_deref(),
            Some("D")
        );
        assert_eq!(key_from_name("MP2_133_Bbm_Guitar").as_deref(), Some("A#m"));
        assert_eq!(key_from_name("Pad_Cmaj").as_deref(), Some("C"));
        assert_eq!(key_from_name("Lead Ebm").as_deref(), Some("D#m"));
    }

    #[test]
    fn no_key_from_a_lone_letter_without_tempo() {
        assert_eq!(key_from_name("Kick_A"), None);
        assert_eq!(key_from_name("Snare B 02"), None);
        assert_eq!(key_from_name("a lire"), None);
        assert_eq!(key_from_name("Bass_Long"), None);
    }

    #[test]
    fn tempo_from_whole_bars() {
        // 32 beats at 100 BPM last 19.2 s.
        assert_eq!(tempo_from_length(19.2), Some(100));
        // 8 beats at 120 BPM: 4 s.
        assert_eq!(tempo_from_length(4.0), Some(120));
        // A one-shot.
        assert_eq!(tempo_from_length(0.437), None);
        // A length that is no whole number of beats at a usual tempo.
        assert_eq!(tempo_from_length(3.17), None);
    }

    #[test]
    fn name_before_length() {
        assert_eq!(
            tempo("Loop 90bpm", Some(4.0), None),
            Some((90, Source::Name))
        );
        assert_eq!(tempo("Loop", Some(4.0), None), Some((120, Source::Length)));
        assert_eq!(tempo("Kick", Some(0.3), None), None);
    }

    #[test]
    fn loop_words() {
        use std::path::Path;
        assert_eq!(
            loop_from_words(Path::new("/s/Arab Percussion Loop 1.wav")),
            Some(true)
        );
        assert_eq!(
            loop_from_words(Path::new("/s/Dub Loops/HITS/Kick/kick_01.wav")),
            Some(false)
        );
        assert_eq!(
            loop_from_words(Path::new("/s/Loops/Kick One Shot.wav")),
            Some(false)
        );
        assert_eq!(
            loop_from_words(Path::new("/s/Loops/Oneshots/tom.wav")),
            Some(false)
        );
        assert_eq!(
            loop_from_words(Path::new("/s/Pack/Drum Loops/beat.wav")),
            Some(true)
        );
        assert_eq!(
            loop_from_words(Path::new("/s/MP2_FX/MP2_Plane.wav")),
            Some(false)
        );
        assert_eq!(
            loop_from_words(Path::new("/s/MP2_DRUM_FILLS/MP2_DrumFill_16.wav")),
            Some(false)
        );
        // "loophole" or "hither" are not the words.
        assert_eq!(loop_from_words(Path::new("/s/loophole/hither.wav")), None);
    }

    #[test]
    fn loop_sources() {
        use std::path::Path;
        let kick = Path::new("/s/Kick.wav");
        assert_eq!(looped(kick, Some(true), false), (true, Source::File));
        assert_eq!(
            looped(Path::new("/s/Hits/x.wav"), None, true),
            (false, Source::Name)
        );
        assert_eq!(looped(kick, None, true), (true, Source::Length));
        assert_eq!(looped(kick, None, false), (false, Source::Length));
    }

    #[test]
    fn name_then_acid_then_length() {
        // The name wins, then the ACID beats (8 beats in 5 s: 96), then the
        // length (which would guess 96 too here, but as an estimate).
        assert_eq!(
            tempo("Loop 90bpm", Some(5.0), Some(8)),
            Some((90, Source::Name))
        );
        assert_eq!(tempo("Loop", Some(5.0), Some(8)), Some((96, Source::File)));
        assert_eq!(
            tempo("Loop", Some(5.0), Some(0)),
            Some((96, Source::Length))
        );
    }

    #[test]
    fn keys_typed_by_hand() {
        assert_eq!(normalize_key("bbm").as_deref(), Some("A#m"));
        assert_eq!(normalize_key(" am ").as_deref(), Some("Am"));
        assert_eq!(normalize_key("F#").as_deref(), Some("F#"));
        assert_eq!(normalize_key("H"), None);
    }

    /// Pseudo-random strings, the same at each run, from pieces a sample's
    /// name may hold: separators, digits, notes, "bpm", accents, emoji.
    fn random_names(count: usize) -> Vec<String> {
        const PIECES: &[&str] = &[
            "_",
            " ",
            "-",
            "(",
            ")",
            ".",
            ",",
            "[",
            "]",
            "1",
            "2",
            "0",
            "9",
            "120",
            "50",
            "250",
            "251",
            "49",
            "bpm",
            "BPM",
            "Bpm",
            "A",
            "Bb",
            "C#",
            "m",
            "min",
            "maj",
            "major",
            "b",
            "#",
            "Kick",
            "Loop",
            "é",
            "e\u{301}",
            "🎹",
            "İ",
            "ß",
            "１２０",
            "\u{0}",
            "Cb",
            "E#",
        ];
        let mut state: u32 = 7;
        (0..count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let length = (state >> 28) as usize;
                (0..length)
                    .map(|_| {
                        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        PIECES[(state >> 8) as usize % PIECES.len()]
                    })
                    .collect()
            })
            .collect()
    }

    fn is_a_key(key: &str) -> bool {
        let note = key.strip_suffix('m').unwrap_or(key);
        NOTES.contains(&note)
    }

    #[test]
    fn names_never_give_a_tempo_or_key_out_of_their_range() {
        for name in random_names(20_000) {
            if let Some(tempo) = tempo_from_name(&name) {
                assert!(TEMPOS.contains(&tempo), "{name:?}: {tempo}");
            }
            if let Some(key) = key_from_name(&name) {
                assert!(is_a_key(&key), "{name:?}: {key}");
                assert_eq!(normalize_key(&key).as_deref(), Some(key.as_str()));
            }
            if let Some(key) = normalize_key(&name) {
                assert!(is_a_key(&key), "{name:?}: {key}");
            }
        }
    }

    #[test]
    fn bpm_in_any_case() {
        assert_eq!(tempo_from_name("Loop 120Bpm"), Some(120));
        assert_eq!(tempo_from_name("Loop 120 BpM"), Some(120));
        assert_eq!(tempo_from_name("Loop_120_BPM"), Some(120));
        assert_eq!(tempo_from_name("Loop (120bPm)"), Some(120));
    }

    #[test]
    fn tempos_in_names_at_the_edges_of_their_range() {
        assert_eq!(tempo_from_name("Loop 49bpm"), None);
        assert_eq!(tempo_from_name("Loop 50bpm"), Some(50));
        assert_eq!(tempo_from_name("Loop 250 BPM"), Some(250));
        assert_eq!(tempo_from_name("Loop 251bpm"), None);
        assert_eq!(tempo_from_name("Loop 1000bpm"), None);
        assert_eq!(tempo_from_name("Loop 9bpm"), None);
        assert_eq!(tempo_from_name("Loop_300_Gm"), None);
        assert_eq!(tempo_from_name("Loop_0bpm"), None);
        assert_eq!(tempo_from_name("bpm"), None);
        assert_eq!(tempo_from_name(""), None);
    }

    #[test]
    fn several_numbers_in_a_name() {
        assert_eq!(tempo_from_name("Pack 2 Loop 03 128bpm 44"), Some(128));
        // The written tempo wins over a bare number before a key.
        assert_eq!(tempo_from_name("Loop_140_Gm_120bpm"), Some(120));
        assert_eq!(tempo_from_name("MP2_84_Drums_120_Am"), Some(120));
        assert_eq!(tempo_from_name("Vol 3 Kick 808 909"), None);
    }

    #[test]
    fn numbers_glued_to_letters_are_no_tempo() {
        assert_eq!(tempo_from_name("Kick120"), None);
        assert_eq!(tempo_from_name("V120_Gm"), None);
        assert_eq!(tempo_from_name("120Gm"), None);
        assert_eq!(key_from_name("Kick120"), None);
    }

    #[test]
    fn unicode_in_names() {
        assert_eq!(tempo_from_name("Café 🎹 120bpm Am"), Some(120));
        assert_eq!(key_from_name("Café 🎹 120bpm Am").as_deref(), Some("Am"));
        assert_eq!(tempo_from_name("Été_120_Dm"), Some(120));
        // Decomposed accents (NFD, as HFS+ writes them).
        assert_eq!(tempo_from_name("Re\u{301}gion_120_Dm"), Some(120));
        assert_eq!(key_from_name("Re\u{301}gion_120_Dm").as_deref(), Some("Dm"));
        // "É" decomposed starts with an E, but is no key.
        assert_eq!(key_from_name("Loop 120bpm E\u{301}te\u{301}"), None);
        // Digits of other scripts are not a tempo.
        assert_eq!(tempo_from_name("Loop １２０bpm"), None);
        assert_eq!(tempo_from_name("Loop ١٢٠ bpm"), None);
        assert_eq!(tempo_from_name("🎹120bpm"), None);
        assert_eq!(key_from_name("B♭m 120bpm"), None);
    }

    #[test]
    fn keys_in_flats_and_sharps() {
        let key = |name: &str| key_from_name(&format!("Loop 120bpm {name}"));
        assert_eq!(key("Cb").as_deref(), Some("B"));
        assert_eq!(key("Fb").as_deref(), Some("E"));
        assert_eq!(key("B#").as_deref(), Some("C"));
        assert_eq!(key("E#").as_deref(), Some("F"));
        assert_eq!(key("Abmin").as_deref(), Some("G#m"));
        assert_eq!(key("Gbmajor").as_deref(), Some("F#"));
        assert_eq!(key("Dminor").as_deref(), Some("Dm"));
        assert_eq!(key("C##"), None);
        assert_eq!(key("Cbb"), None);
        assert_eq!(key("H"), None);
        // Lower case is a word, not a key, even next to a tempo.
        assert_eq!(key("am"), None);
        assert_eq!(tempo_from_name("Loop_120_am"), None);
    }

    #[test]
    fn a_key_with_a_quality_wins_over_a_lone_letter_elsewhere() {
        // Expected Gm: "A" is not next to the tempo, "Gm" is.
        assert_eq!(key_from_name("Kit_A_Loop_120_Gm").as_deref(), Some("Gm"));
    }

    #[test]
    fn keys_typed_by_hand_in_odd_spellings() {
        for key in NOTES {
            assert_eq!(normalize_key(key).as_deref(), Some(key));
            let minor = format!("{key}m");
            assert_eq!(normalize_key(&minor), Some(minor.clone()));
            assert_eq!(normalize_key(&minor.to_lowercase()), Some(minor));
        }
        assert_eq!(normalize_key(""), None);
        assert_eq!(normalize_key("   "), None);
        assert_eq!(normalize_key("é"), None);
        assert_eq!(normalize_key("ébm"), None);
        assert_eq!(normalize_key("🎹"), None);
        assert_eq!(normalize_key("b♭"), None);
        assert_eq!(normalize_key("\u{0}"), None);
    }

    #[test]
    fn tempo_from_odd_lengths() {
        for seconds in [
            0.0,
            -0.0,
            -4.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN_POSITIVE,
            5e-324,
            f64::MAX,
        ] {
            assert_eq!(tempo_from_length(seconds), None, "{seconds}");
            assert_eq!(tempo("Loop", Some(seconds), Some(8)), None, "{seconds}");
        }
    }

    #[test]
    fn tempo_from_length_stays_in_its_range() {
        let beats = [4, 8, 12, 16, 24, 32, 48, 64, 96, 128];
        for millis in 1..200_000 {
            let seconds = f64::from(millis) / 1000.0;
            let Some(tempo) = tempo_from_length(seconds) else {
                continue;
            };
            assert!((70..=180).contains(&tempo), "{seconds} s: {tempo}");
            // A whole number of the beats looked at, at that tempo.
            assert!(
                beats.iter().any(|b| {
                    let exact = 60.0 * f64::from(*b) / seconds;
                    exact.round() as u32 == tempo
                }),
                "{seconds} s: {tempo}"
            );
        }
    }

    #[test]
    fn tempo_from_length_at_the_edges_of_its_range() {
        // 4 beats at 180 BPM; 8 beats would be 360.
        assert_eq!(tempo_from_length(60.0 * 4.0 / 180.0), Some(180));
        assert_eq!(tempo_from_length(60.0 * 4.0 / 181.0), None);
        // 96 beats at 70 BPM, no other count of beats giving a whole tempo.
        assert_eq!(tempo_from_length(60.0 * 96.0 / 70.0), Some(70));
        assert_eq!(tempo_from_length(60.0 * 128.0 / 69.0), None);
        // 128 beats, the most looked at.
        assert_eq!(tempo_from_length(60.0 * 128.0 / 75.0), Some(75));
    }

    #[test]
    fn odd_acid_beats_fall_back_to_the_length() {
        assert_eq!(
            tempo("Loop", Some(4.0), Some(0)),
            Some((120, Source::Length))
        );
        assert_eq!(
            tempo("Loop", Some(4.0), Some(u32::MAX)),
            Some((120, Source::Length))
        );
        assert_eq!(tempo("Loop", None, Some(8)), None);
        // The chunk's tempo may lie outside the length's range.
        assert_eq!(tempo("Loop", Some(24.0), Some(8)), Some((20, Source::File)));
    }

    #[test]
    fn loop_words_in_odd_paths() {
        use std::path::Path;
        assert_eq!(loop_from_words(Path::new("")), None);
        assert_eq!(loop_from_words(Path::new("/")), None);
        assert_eq!(loop_from_words(Path::new("/s/.wav")), None);
        assert_eq!(
            loop_from_words(Path::new("/s/Kick One-Shot.wav")),
            Some(false)
        );
        assert_eq!(
            loop_from_words(Path::new("/s/ONE_SHOTS/x.wav")),
            Some(false)
        );
        // One-shot words win over loop words in the same part.
        assert_eq!(loop_from_words(Path::new("/s/Loop Hits.wav")), Some(false));
        assert_eq!(loop_from_words(Path::new("/s/🎹 Loop é.wav")), Some(true));
        assert_eq!(loop_from_words(Path::new("/s/LOOPS/x.wav")), Some(true));
    }

    #[cfg(unix)]
    #[test]
    fn loop_words_in_a_path_not_in_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::Path;
        let path = Path::new(OsStr::from_bytes(b"/s/Loops/\xff\xfe.wav"));
        assert_eq!(loop_from_words(path), Some(true));
        assert_eq!(looped(path, None, false), (true, Source::Name));
    }
}
