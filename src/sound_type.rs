// The kind of sound a sample is, from the words of its file name and
// folders. Two levels: a family (Drums, Bass, Brass & Woodwinds…) and, when
// the words say it, a subtype (Drums › kick). The list merges LANDR's and
// Splice's instrument categories, as chosen for samplr; "Music", a whole
// piece rather than a sample, comes from Freesound's Broad Sound Taxonomy.
//
// A type is written `family/subtype` ("drums/kick") or `family` ("brass-
// woodwinds"): a subtype's id may be found in several families ("electric"
// in Guitar and Bass, "synth" in Guitar and Strings).

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use serde::Serialize;

#[derive(Serialize)]
pub struct Subtype {
    pub id: &'static str,
    pub label: &'static str,
    /// Words, or two-word phrases ("closed hat"), saying this subtype
    /// whatever the family around them.
    #[serde(skip)]
    words: &'static [&'static str],
}

#[derive(Serialize)]
pub struct Family {
    pub id: &'static str,
    pub label: &'static str,
    /// Words saying the family only.
    #[serde(skip)]
    words: &'static [&'static str],
    pub subtypes: &'static [Subtype],
}

const fn sub(id: &'static str, label: &'static str, words: &'static [&'static str]) -> Subtype {
    Subtype { id, label, words }
}

pub const FAMILIES: &[Family] = &[
    Family {
        id: "drums",
        label: "Drums",
        words: &[
            "drums", "drum", "beat", "beats", "groove", "grooves", "top", "tops",
        ],
        subtypes: &[
            sub(
                "kick",
                "Kick",
                &["kick", "kicks", "bd", "bassdrum", "bass drum"],
            ),
            sub("snare", "Snare", &["snare", "snares", "sd"]),
            sub(
                "rimshot",
                "Rimshot",
                &["rim", "rims", "rimshot", "rimshots"],
            ),
            sub("clap", "Clap", &["clap", "claps"]),
            sub(
                "hi-hat",
                "Hi-hat",
                &["hat", "hats", "hihat", "hihats", "hh", "hi hat"],
            ),
            sub(
                "closed-hi-hat",
                "Closed hi-hat",
                &["closed hat", "closed hihat", "closed hh", "chh"],
            ),
            sub(
                "open-hi-hat",
                "Open hi-hat",
                &["open hat", "open hihat", "open hh", "ohh"],
            ),
            sub("tom", "Tom", &["tom", "toms"]),
            sub("cymbals", "Cymbals", &["cymbal", "cymbals"]),
            sub("crash", "Crash", &["crash", "crashes"]),
            sub("ride", "Ride", &["ride", "rides"]),
            sub("splash", "Splash", &["splash"]),
            sub(
                "full-drum",
                "Full drum",
                &["full drum", "full kit", "drum kit"],
            ),
            sub(
                "breaks",
                "Breaks",
                &["break", "breaks", "breakbeat", "breakbeats"],
            ),
            sub("fills", "Fills", &["fill", "fills", "roll", "rolls"]),
        ],
    },
    Family {
        id: "bass",
        label: "Bass",
        words: &["bass", "basses", "bassline", "basslines", "808"],
        subtypes: &[
            sub(
                "bass-guitar",
                "Bass guitar",
                &["bass guitar", "electric bass"],
            ),
            sub(
                "upright-bass",
                "Upright bass",
                &["upright bass", "acoustic bass"],
            ),
            sub("sub", "Sub", &["sub", "subs", "subbass", "sub bass"]),
            sub(
                "synth-bass",
                "Synth bass",
                &["synth bass", "synthbass", "reese"],
            ),
        ],
    },
    Family {
        id: "brass-woodwinds",
        label: "Brass & Woodwinds",
        words: &[
            "brass",
            "woodwind",
            "woodwinds",
            "winds",
            "bagpipe",
            "bagpipes",
        ],
        subtypes: &[
            sub(
                "saxophone",
                "Saxophone",
                &["sax", "saxes", "saxophone", "saxophones"],
            ),
            sub("trumpet", "Trumpet", &["trumpet", "trumpets"]),
            sub("trombone", "Trombone", &["trombone", "trombones"]),
            sub("tuba", "Tuba", &["tuba", "tubas"]),
            sub("horn", "Horn", &["horn", "horns"]),
            sub(
                "french-horn",
                "French horn",
                &["french horn", "french horns"],
            ),
            sub(
                "english-horn",
                "English horn",
                &["english horn", "cor anglais"],
            ),
            sub("flute", "Flute", &["flute", "flutes"]),
            sub("clarinet", "Clarinet", &["clarinet", "clarinets"]),
            sub("oboe", "Oboe", &["oboe", "oboes"]),
            sub("bassoon", "Bassoon", &["bassoon", "bassoons"]),
            sub("harmonica", "Harmonica", &["harmonica"]),
            sub("didgeridoo", "Didgeridoo", &["didgeridoo"]),
            sub("ensemble", "Ensemble", &["horn section", "brass section"]),
        ],
    },
    Family {
        id: "guitar",
        label: "Guitar",
        words: &[
            "guitar", "guitars", "gtr", "lute", "lutes", "skank", "skanks", "riff", "riffs",
        ],
        subtypes: &[
            sub("electric", "Electric", &["electric guitar"]),
            sub("acoustic", "Acoustic", &["acoustic guitar"]),
            sub("12-string", "12 string", &["12 string"]),
            sub(
                "steel-guitar",
                "Steel guitar",
                &["steel guitar", "pedal steel", "lap steel"],
            ),
            sub("banjo", "Banjo", &["banjo"]),
            sub("mandolin", "Mandolin", &["mandolin"]),
            sub("ukulele", "Ukulele", &["ukulele", "uke"]),
            sub("oud", "Oud", &["oud"]),
            sub("sitar", "Sitar", &["sitar"]),
            sub("koto", "Koto", &["koto"]),
            sub("autoharp", "Autoharp", &["autoharp"]),
            sub("synth", "Synth", &["synth guitar"]),
        ],
    },
    Family {
        id: "keys",
        label: "Keys",
        words: &["keys", "chord", "chords"],
        subtypes: &[
            sub("piano", "Piano", &["piano", "pianos"]),
            sub(
                "electric-piano",
                "Electric piano",
                &["electric piano", "epiano"],
            ),
            sub("rhodes", "Rhodes", &["rhodes"]),
            sub("wurlitzer", "Wurlitzer", &["wurlitzer", "wurli"]),
            sub("organ", "Organ", &["organ", "organs"]),
            sub("clavinet", "Clavinet", &["clavinet", "clav"]),
            sub("harpsichord", "Harpsichord", &["harpsichord"]),
            sub("celeste", "Celeste", &["celeste", "celesta"]),
            sub("accordion", "Accordion", &["accordion"]),
        ],
    },
    Family {
        id: "synth",
        label: "Synth",
        words: &["synth", "synths"],
        subtypes: &[
            sub("lead", "Lead", &["lead", "leads"]),
            sub("pad", "Pad", &["pad", "pads"]),
            sub("pluck", "Pluck", &["pluck", "plucks", "plucked"]),
            sub("stab", "Stab", &["stab", "stabs"]),
            sub("arp", "Arp", &["arp", "arps", "arpeggio"]),
            sub("bass", "Bass", &[]),
            sub("bell", "Bell", &[]),
            sub("texture", "Texture", &[]),
            sub("fx", "Fx", &[]),
        ],
    },
    Family {
        id: "strings",
        label: "Strings",
        words: &["strings"],
        subtypes: &[
            sub("violin", "Violin", &["violin", "violins", "fiddle"]),
            sub("viola", "Viola", &["viola", "violas"]),
            sub("cello", "Cello", &["cello", "cellos"]),
            sub("double-bass", "Double bass", &["double bass", "contrabass"]),
            sub("harp", "Harp", &["harp", "harps"]),
            sub("quartet", "Quartet", &["quartet", "string quartet"]),
            sub("ensemble", "Ensemble", &["string section"]),
            sub("orchestral", "Orchestral", &["orchestral", "orchestra"]),
            sub("synth", "Synth", &["synth strings"]),
        ],
    },
    Family {
        id: "percussion",
        label: "Percussion",
        words: &[
            "perc",
            "percs",
            "percussion",
            "percussions",
            "darbouka",
            "darbuka",
            "bendir",
            "bowls",
        ],
        subtypes: &[
            sub(
                "shaker",
                "Shaker",
                &["shaker", "shakers", "sheker", "shekers"],
            ),
            sub(
                "tambourine",
                "Tambourine",
                &["tambourine", "tambourines", "tamb", "tambo"],
            ),
            sub("conga", "Conga", &["conga", "congas"]),
            sub("bongo", "Bongo", &["bongo", "bongos"]),
            sub("djembe", "Djembe", &["djembe", "djembes"]),
            sub("tabla", "Tabla", &["tabla", "tablas"]),
            sub("timbale", "Timbale", &["timbale", "timbales"]),
            sub("cowbell", "Cowbell", &["cowbell", "cowbells"]),
            sub(
                "woodblock",
                "Woodblock",
                &["woodblock", "woodblocks", "wood block"],
            ),
            sub("claves", "Claves", &["clave", "claves"]),
            sub("cabasa", "Cabasa", &["cabasa"]),
            sub("triangle", "Triangle", &["triangle", "triangles"]),
            sub("gong", "Gong", &["gong", "gongs"]),
            sub("bells", "Bells", &["bell", "bells"]),
            sub("snap", "Snap", &["snap", "snaps"]),
            sub("wooden", "Wooden", &["wooden"]),
        ],
    },
    Family {
        id: "pitched-percussion",
        label: "Pitched Percussion",
        words: &["mallets"],
        subtypes: &[
            sub("marimba", "Marimba", &["marimba"]),
            sub("xylophone", "Xylophone", &["xylophone"]),
            sub("vibraphone", "Vibraphone", &["vibraphone", "vibes"]),
            sub("glockenspiel", "Glockenspiel", &["glockenspiel", "glock"]),
            sub("kalimba", "Kalimba", &["kalimba", "mbira", "thumb piano"]),
            sub(
                "steel-drum",
                "Steel drum",
                &["steel drum", "steel drums", "steelpan"],
            ),
            sub("tubular-bells", "Tubular bells", &["tubular bells"]),
            sub("chime", "Chime", &["chime", "chimes"]),
            sub("crotales", "Crotales", &["crotales"]),
        ],
    },
    Family {
        id: "vocals",
        label: "Vocals",
        words: &["vocal", "vocals", "vox", "voice", "voices"],
        subtypes: &[
            sub("female", "Female", &["female"]),
            sub("male", "Male", &["male"]),
            sub("child", "Child", &["child", "children", "kid", "kids"]),
            sub("choir", "Choir", &["choir", "choirs"]),
            sub("singing", "Singing", &["singing", "sung"]),
            sub("topline", "Topline", &["topline", "toplines"]),
            sub("chant", "Chant", &["chant", "chants"]),
            sub("rap", "Rap", &["rap", "raps"]),
            sub("spoken", "Spoken", &["spoken"]),
            sub("dialogue", "Dialogue", &["dialogue", "interview"]),
            sub("phrases", "Phrases", &["phrase", "phrases"]),
            sub("shout", "Shout", &["shout", "shouts"]),
            sub("scream", "Scream", &["scream", "screams"]),
            sub("whisper", "Whisper", &["whisper", "whispers"]),
            sub("beatbox", "Beatbox", &["beatbox"]),
        ],
    },
    Family {
        id: "fx",
        label: "FX",
        words: &[
            "fx", "sfx", "siren", "laser", "scratch", "alarm", "klaxon", "weird",
        ],
        subtypes: &[
            sub("riser", "Riser", &["riser", "risers", "uplifter"]),
            sub("downer", "Downer", &["downer", "downers", "downlifter"]),
            sub("sweep", "Sweep", &["sweep", "sweeps"]),
            sub("impact", "Impact", &["impact", "impacts", "hit"]),
            sub("noise", "Noise", &["noise", "noises"]),
            sub("texture", "Texture", &["texture", "textures"]),
            sub(
                "atmosphere",
                "Atmosphere",
                &[
                    "atmosphere",
                    "atmos",
                    "ambience",
                    "soundscape",
                    "scape",
                    "dubscapes",
                    "drone",
                ],
            ),
            sub(
                "field-recording",
                "Field recording",
                &["field recording", "field recordings"],
            ),
            sub(
                "sound-effect",
                "Sound effect",
                &["sound effect", "sound effects"],
            ),
        ],
    },
    Family {
        id: "music",
        label: "Music",
        words: &["mix", "master", "song", "songs"],
        subtypes: &[],
    },
];

/// Words that say a type only when nothing else does: "Horns_Skank" is
/// brass, "Elec_Gtr_Chords" guitar, "Sax_Lead" brass, "Kick_Drum" a kick.
const WEAK: &[&str] = &[
    "drums", "drum", "beat", "beats", "top", "tops", "skank", "skanks", "chord", "chords", "riff",
    "riffs", "lead", "leads", "pluck", "plucks", "plucked", "stab", "stabs", "hit", "mix",
    "master",
];

/// Words that give a subtype only within a family known otherwise:
/// "Elec_Gtr" is an electric guitar, "Synth_Strings" synth strings.
const QUALIFIERS: &[(&str, &str, &str)] = &[
    ("electric", "guitar", "electric"),
    ("elec", "guitar", "electric"),
    ("acoustic", "guitar", "acoustic"),
    ("synth", "guitar", "synth"),
    ("electric", "bass", "bass-guitar"),
    ("elec", "bass", "bass-guitar"),
    ("acoustic", "bass", "upright-bass"),
    ("upright", "bass", "upright-bass"),
    ("synth", "bass", "synth-bass"),
    ("electric", "keys", "electric-piano"),
    ("ensemble", "brass-woodwinds", "ensemble"),
    ("section", "brass-woodwinds", "ensemble"),
    ("ensemble", "strings", "ensemble"),
    ("section", "strings", "ensemble"),
    ("synth", "strings", "synth"),
    ("bass", "synth", "bass"),
    ("bell", "synth", "bell"),
    ("texture", "synth", "texture"),
    ("fx", "synth", "fx"),
];

/// What a word or phrase says.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Family(&'static Family),
    Subtype(&'static Family, &'static Subtype),
}

impl PartialEq for Family {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Debug for Family {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.id)
    }
}

impl PartialEq for Subtype {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Debug for Subtype {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.id)
    }
}

impl Hit {
    fn family(self) -> &'static Family {
        match self {
            Hit::Family(family) | Hit::Subtype(family, _) => family,
        }
    }

    fn value(self) -> String {
        match self {
            Hit::Family(family) => family.id.to_string(),
            Hit::Subtype(family, subtype) => format!("{}/{}", family.id, subtype.id),
        }
    }
}

/// Every word of `FAMILIES` with what it says, built once: the search works
/// out the type of every sample, and walking the whole list for each word
/// made it the slowest part of a search. A word met twice keeps its first
/// meaning, in the order of `FAMILIES`.
static HITS: LazyLock<HashMap<&'static str, Hit>> = LazyLock::new(|| {
    let mut hits = HashMap::new();
    for family in FAMILIES {
        for word in family.words {
            hits.entry(*word).or_insert(Hit::Family(family));
        }
        for subtype in family.subtypes {
            for word in subtype.words {
                hits.entry(*word).or_insert(Hit::Subtype(family, subtype));
            }
        }
    }
    hits
});

fn hit_of(word: &str) -> Option<Hit> {
    HITS.get(word).copied()
}

/// The words of a name or folder, a two-word phrase taken whole when it
/// says something ("closed hat", "double bass").
fn tokens(part: &str) -> Vec<String> {
    let lower = part.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if let Some(next) = words.get(i + 1) {
            let phrase = format!("{} {next}", words[i]);
            if hit_of(&phrase).is_some() {
                tokens.push(phrase);
                i += 2;
                continue;
            }
        }
        tokens.push(words[i].to_string());
        i += 1;
    }
    tokens
}

/// What the tokens of a part say, in their order; the weak words only when
/// no other one says anything.
fn hits(tokens: &[String]) -> Vec<Hit> {
    let strong: Vec<Hit> = tokens
        .iter()
        .filter(|token| !WEAK.contains(&token.as_str()))
        .filter_map(|token| hit_of(token))
        .collect();
    if !strong.is_empty() {
        return strong;
    }
    tokens.iter().filter_map(|token| hit_of(token)).collect()
}

/// The last subtype of `family` the tokens say, by its own words or by a
/// qualifier.
fn subtype_in(tokens: &[String], family: &'static Family) -> Option<Hit> {
    tokens.iter().rev().find_map(|token| {
        if let Some(Hit::Subtype(found, subtype)) = hit_of(token)
            && found == family
        {
            return Some(Hit::Subtype(family, subtype));
        }
        let (_, _, subtype_id) = QUALIFIERS
            .iter()
            .find(|(word, family_id, _)| word == token && *family_id == family.id)?;
        let subtype = family.subtypes.iter().find(|s| s.id == *subtype_id)?;
        Some(Hit::Subtype(family, subtype))
    })
}

/// The sample's type from its path: the file name, then its folders from
/// the deepest up. In a name the last word saying a type wins, the main noun
/// coming last in English ("Atmos Flute" is a flute). When it only gives a
/// family, the same name or the folders above may still give a subtype of
/// that family (`HITS/Kick/MP2_Drums_1.wav` is a kick, `Elec_Gtr` an
/// electric guitar). `None` when nothing says it.
pub fn from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let folders = path
        .parent()
        .into_iter()
        .flat_map(|folder| folder.iter().rev())
        .filter_map(|part| part.to_str());
    let mut found: Option<&'static Family> = None;
    for part in std::iter::once(stem).chain(folders) {
        let tokens = tokens(part);
        if let Some(family) = found {
            if let Some(hit) = subtype_in(&tokens, family) {
                return Some(hit.value());
            }
            continue;
        }
        let Some(&last) = hits(&tokens).last() else {
            continue;
        };
        if let Hit::Subtype(..) = last {
            return Some(last.value());
        }
        let family = last.family();
        if let Some(hit) = subtype_in(&tokens, family) {
            return Some(hit.value());
        }
        found = Some(family);
    }
    found.map(|family| family.id.to_string())
}

/// A stored or typed type, checked: `family` or `family/subtype` of
/// `FAMILIES`.
pub fn parse(value: &str) -> Option<(&'static Family, Option<&'static Subtype>)> {
    let (family_id, subtype_id) = match value.split_once('/') {
        Some((family, subtype)) => (family, Some(subtype)),
        None => (value, None),
    };
    let family = FAMILIES.iter().find(|family| family.id == family_id)?;
    match subtype_id {
        None => Some((family, None)),
        Some(id) => Some((family, Some(family.subtypes.iter().find(|s| s.id == id)?))),
    }
}

/// Whether a search word (`type:drums`, `type:kick`, `type:guitar/electric`)
/// names something of the list.
pub fn is_known(query: &str) -> bool {
    parse(query).is_some()
        || FAMILIES
            .iter()
            .any(|family| family.subtypes.iter().any(|subtype| subtype.id == query))
}

/// Whether a sample of type `value` answers the search word `query`: its
/// family, its subtype in any family, or both.
pub fn matches(value: &str, query: &str) -> bool {
    let (family, subtype) = value.split_once('/').unwrap_or((value, ""));
    query == value || query == family || query == subtype
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(path: &str) -> Option<String> {
        from_path(Path::new(path))
    }

    fn is(path: &str, expected: &str) {
        assert_eq!(of(path).as_deref(), Some(expected), "{path}");
    }

    #[test]
    fn name_first() {
        is("/s/Loops/MP2_Kick_1.wav", "drums/kick");
        is(
            "/s/YCE_90_E_Trumpet_Stamper_Wet.wav",
            "brass-woodwinds/trumpet",
        );
    }

    #[test]
    fn last_word_wins() {
        is("/s/Atmos Flute 02 (110BPM).wav", "brass-woodwinds/flute");
        is("/s/Melody Loops/3-Djembe-90BPM-Cm-_guitar-2.wav", "guitar");
    }

    #[test]
    fn weak_words() {
        is("/s/YCE_120_B_Horns_Skank_Wet.wav", "brass-woodwinds/horn");
        is("/s/YCE_90_C_Sax_Lead_Wet.wav", "brass-woodwinds/saxophone");
        is("/s/Kick Drum 3.wav", "drums/kick");
        is("/s/Skanks/Skank_1.wav", "guitar");
    }

    #[test]
    fn phrases_and_qualifiers() {
        is("/s/Closed Hat 01.wav", "drums/closed-hi-hat");
        is("/s/Hats/x_161.wav", "drums/hi-hat");
        is("/s/YCE_70_G_Elec_Gtr_Up_Dry.wav", "guitar/electric");
        is("/s/MP2_94_A_Synth_Strings.wav", "strings/synth");
        is("/s/Double Bass 2.wav", "strings/double-bass");
        is(
            "/s/Thumb Piano 01 (130 BPM).wav",
            "pitched-percussion/kalimba",
        );
        is("/s/Upright Bass 2.wav", "bass/upright-bass");
        // "electric" alone says nothing.
        assert_eq!(of("/s/Electric Dreams.wav"), None);
    }

    #[test]
    fn folders() {
        is(
            "/s/ZionTrain-DubDrums/HITS/Kick/FL_DubDrums_Kick_55.wav",
            "drums/kick",
        );
        // A family in the name, a subtype of it above.
        is("/s/HITS/Kick/MP2_Drums_1.wav", "drums/kick");
        // A subtype of another family above does not count.
        is("/s/Kick/MP2_Bass_1.wav", "bass");
        is("/s/LOOPS/Live drums session 4/FL_006_150bpm.wav", "drums");
        is("/s/ROLLS/FL_DubDrums_011_roll_120bpm.wav", "drums/fills");
    }

    #[test]
    fn unknown() {
        assert_eq!(of("/s/Endless Boogie/02 Whilom.mp3"), None);
        assert_eq!(of("/s/subway.wav"), None);
    }

    #[test]
    fn values() {
        assert!(parse("drums/kick").is_some());
        assert!(parse("drums").is_some());
        assert!(parse("drums/oboe").is_none());
        assert!(is_known("kick"));
        assert!(is_known("electric"));
        assert!(!is_known("kik"));
        assert!(matches("guitar/electric", "electric"));
        assert!(matches("guitar/electric", "guitar"));
        assert!(!matches("guitar/electric", "bass"));
    }
}
