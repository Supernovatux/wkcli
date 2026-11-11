//! Domain types shared by the API layer, the session logic and the UI.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Radical,
    Kanji,
    Vocabulary,
    KanaVocabulary,
}

impl Kind {
    pub fn from_api(s: &str) -> Option<Kind> {
        Some(match s {
            "radical" => Kind::Radical,
            "kanji" => Kind::Kanji,
            "vocabulary" => Kind::Vocabulary,
            "kana_vocabulary" => Kind::KanaVocabulary,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Radical => "Radical",
            Kind::Kanji => "Kanji",
            Kind::Vocabulary | Kind::KanaVocabulary => "Vocabulary",
        }
    }

    pub fn has_reading(self) -> bool {
        matches!(self, Kind::Kanji | Kind::Vocabulary)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QType {
    Meaning,
    Reading,
}

#[derive(Debug, Clone)]
pub struct Meaning {
    pub text: String,
    pub primary: bool,
    pub accepted: bool,
}

#[derive(Debug, Clone)]
pub struct Reading {
    pub text: String,
    pub primary: bool,
    pub accepted: bool,
    /// onyomi / kunyomi / nanori (kanji only)
    pub kind: Option<String>,
}

/// One recording of a vocabulary reading.
#[derive(Debug, Clone)]
pub struct Pronunciation {
    pub url: String,
    /// The reading this recording pronounces.
    pub reading: String,
    pub voice_actor_id: u64,
}

#[derive(Debug, Clone)]
pub struct Subject {
    pub id: u64,
    pub kind: Kind,
    pub characters: Option<String>,
    pub slug: String,
    pub meanings: Vec<Meaning>,
    pub whitelist: Vec<String>,
    pub blacklist: Vec<String>,
    pub synonyms: Vec<String>,
    pub readings: Vec<Reading>,
    pub meaning_mnemonic: String,
    pub reading_mnemonic: String,
    /// SVG source for radicals that have no unicode character.
    pub image_svg: Option<Vec<u8>>,
    /// MP3 recordings (vocabulary only).
    pub audios: Vec<Pronunciation>,
}

impl Subject {
    /// Text used to refer to the subject in lists and logs.
    pub fn display(&self) -> String {
        self.characters
            .clone()
            .unwrap_or_else(|| format!("[{}]", self.slug))
    }
}

#[derive(Debug, Clone)]
pub struct ReviewItem {
    pub assignment_id: u64,
    pub subject: Subject,
}

#[derive(Debug, Clone)]
pub struct Submission {
    pub assignment_id: u64,
    pub label: String,
    pub incorrect_meaning: u32,
    pub incorrect_reading: u32,
    /// Files cached for this subject, removed if the review burns it.
    pub cache_files: Vec<std::path::PathBuf>,
}
