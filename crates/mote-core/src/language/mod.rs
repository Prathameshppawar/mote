//! Deterministic language and script detection for English, Hindi and Marathi,
//! including romanized (Latin-script) and code-mixed text such as Hinglish.
//!
//! Mote uses the result to *preserve* the user's language: prompts sent to the
//! model carry an explicit instruction not to translate, and local spelling
//! correction skips transliterated words instead of "fixing" them into English.

pub mod lexicon;

use serde::{Deserialize, Serialize};

use crate::spelling::dictionary::EnglishDictionary;
use crate::text::{tail_chars, token_in_technical_span, tokenize};

/// The writing system that dominates a piece of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Script {
    Latin,
    Devanagari,
    Mixed,
    Other,
    None,
}

/// The detected language variety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LanguageLabel {
    English,
    /// Hindi written in Latin script, little or no English.
    HindiLatin,
    /// Hindi-dominant text mixed with English, in Latin script.
    Hinglish,
    /// English-dominant text with Hindi words, in Latin script.
    MixedEnglishHindi,
    /// Marathi written in Latin script, little or no English.
    MarathiLatin,
    /// Marathi-dominant text mixed with English, in Latin script.
    MarathiEnglish,
    /// English-dominant text with Marathi words, in Latin script.
    MixedEnglishMarathi,
    HindiDevanagari,
    MarathiDevanagari,
    /// A language Mote does not specifically model.
    Other,
    /// Too little text to decide.
    Unknown,
}

impl LanguageLabel {
    /// Human-readable name for the UI.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::HindiLatin => "Hindi (Latin script)",
            Self::Hinglish => "Hinglish",
            Self::MixedEnglishHindi => "English + Hindi",
            Self::MarathiLatin => "Marathi (Latin script)",
            Self::MarathiEnglish => "Marathi + English",
            Self::MixedEnglishMarathi => "English + Marathi",
            Self::HindiDevanagari => "Hindi",
            Self::MarathiDevanagari => "Marathi",
            Self::Other => "Other language",
            Self::Unknown => "Unknown",
        }
    }

    /// Whether Hindi or Marathi is present in any form.
    pub fn has_indic(self) -> bool {
        !matches!(self, Self::English | Self::Other | Self::Unknown)
    }

    /// Whether Hindi or Marathi is the dominant language.
    pub fn indic_dominant(self) -> bool {
        matches!(
            self,
            Self::HindiLatin
                | Self::Hinglish
                | Self::MarathiLatin
                | Self::MarathiEnglish
                | Self::HindiDevanagari
                | Self::MarathiDevanagari
        )
    }

    /// Instruction appended to model prompts so the model keeps the user's
    /// language and script instead of translating.
    pub fn preservation_instruction(self) -> &'static str {
        match self {
            Self::English => "Write in English.",
            Self::HindiLatin => "The text is Hindi written in Latin script. Keep Hindi in Latin script. Do not translate it and do not switch to Devanagari.",
            Self::Hinglish => "The text is Hinglish (Hindi mixed with English, Latin script). Keep the same Hinglish mix and Latin script. Do not translate it into English.",
            Self::MixedEnglishHindi => "The text is English mixed with Hindi words in Latin script. Keep the Hindi words exactly as they are. Do not translate them.",
            Self::MarathiLatin => "The text is Marathi written in Latin script. Keep Marathi in Latin script. Do not translate it into English or Hindi.",
            Self::MarathiEnglish => "The text is Marathi mixed with English, in Latin script. Keep the same Marathi-English mix and Latin script. Do not translate it.",
            Self::MixedEnglishMarathi => "The text is English mixed with Marathi words in Latin script. Keep the Marathi words exactly as they are. Do not translate them.",
            Self::HindiDevanagari => "The text is Hindi in Devanagari script. Keep Hindi in Devanagari.",
            Self::MarathiDevanagari => "The text is Marathi in Devanagari script. Keep Marathi in Devanagari.",
            Self::Other => "Keep the same language and script as the text. Do not translate.",
            Self::Unknown => "Match the language and script of the text. Do not translate.",
        }
    }
}

/// Result of language detection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LanguageProfile {
    pub label: LanguageLabel,
    pub script: Script,
    pub english_share: f32,
    pub hindi_share: f32,
    pub marathi_share: f32,
    /// 0.0..=1.0; grows with the amount of text and the margin of the decision.
    pub confidence: f32,
    pub token_count: u32,
}

impl LanguageProfile {
    /// Profile for empty or unclassifiable input.
    pub fn unknown() -> Self {
        Self {
            label: LanguageLabel::Unknown,
            script: Script::None,
            english_share: 0.0,
            hindi_share: 0.0,
            marathi_share: 0.0,
            confidence: 0.0,
            token_count: 0,
        }
    }

    /// Profile for text known to be English (used by tests and defaults).
    pub fn english() -> Self {
        Self {
            label: LanguageLabel::English,
            script: Script::Latin,
            english_share: 1.0,
            confidence: 1.0,
            ..Self::unknown()
        }
    }
}

/// Maximum characters examined; detection only needs recent context.
const SAMPLE_CHARS: usize = 1_200;
/// Below this share of Indic tokens, Latin text is English regardless of markers.
const INDIC_TRACE_SHARE: f32 = 0.05;
/// Below this share, Latin text is English unless a distinctive Indic word appears.
const INDIC_MIN_SHARE: f32 = 0.12;
/// Without function words, Indic counts as the frame language above this share.
const INDIC_DOMINANT_SHARE: f32 = 0.40;
/// Indic-framed text with less English than this is pure transliteration.
const PURE_MAX_ENGLISH_SHARE: f32 = 0.25;

fn is_devanagari(c: char) -> bool {
    ('\u{0900}'..='\u{097F}').contains(&c)
}

/// Detects the language variety of `text`.
pub fn detect(text: &str) -> LanguageProfile {
    let sample = tail_chars(text, SAMPLE_CHARS);
    let (mut latin, mut devanagari, mut other) = (0usize, 0usize, 0usize);
    for c in sample.chars().filter(|c| c.is_alphabetic()) {
        if c.is_ascii_alphabetic() || ('\u{00C0}'..='\u{024F}').contains(&c) {
            latin += 1;
        } else if is_devanagari(c) {
            devanagari += 1;
        } else {
            other += 1;
        }
    }
    let letters = latin + devanagari + other;
    if letters == 0 {
        return LanguageProfile::unknown();
    }
    let ratio = |n: usize| n as f32 / letters as f32;
    let script = if ratio(devanagari) >= 0.2 && ratio(latin) >= 0.2 {
        Script::Mixed
    } else if ratio(devanagari) > 0.5 {
        Script::Devanagari
    } else if ratio(latin) > 0.5 {
        Script::Latin
    } else {
        Script::Other
    };

    if ratio(devanagari) >= 0.5 {
        return detect_devanagari(sample, script);
    }
    if ratio(other) > 0.5 {
        return LanguageProfile { label: LanguageLabel::Other, script, confidence: 0.6, ..LanguageProfile::unknown() };
    }
    detect_latin(sample, script)
}

fn detect_devanagari(sample: &str, script: Script) -> LanguageProfile {
    let mut hindi = 0.0f32;
    let mut marathi = 0.0f32;
    let mut tokens = 0u32;
    for word in sample.split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '?' | '!' | '।')) {
        if word.is_empty() {
            continue;
        }
        tokens += 1;
        if lexicon::HINDI_DEVANAGARI_MARKERS.contains(&word) {
            hindi += 1.0;
        }
        if lexicon::MARATHI_DEVANAGARI_MARKERS.contains(&word) {
            marathi += 1.0;
        }
    }
    // The retroflex lateral ळ is frequent in Marathi and rare in Hindi.
    marathi += (sample.matches('ळ').count() as f32 * 0.5).min(1.5);
    let is_marathi = marathi > hindi;
    let total = (hindi + marathi).max(1.0);
    LanguageProfile {
        label: if is_marathi { LanguageLabel::MarathiDevanagari } else { LanguageLabel::HindiDevanagari },
        script,
        english_share: 0.0,
        hindi_share: if is_marathi { hindi / total } else { 1.0 - marathi / total },
        marathi_share: if is_marathi { 1.0 - hindi / total } else { marathi / total },
        confidence: (0.5 + (hindi - marathi).abs() / total * 0.5).min(1.0) * (tokens as f32 / 4.0).min(1.0),
        token_count: tokens,
    }
}

/// English grammatical function words. Together with [`INDIC_FUNCTION_WORDS`]
/// they decide the *matrix* language of code-mixed text: "meeting ka time
/// change kar do" is Hinglish because its grammar is Hindi, even though half of
/// its words are English nouns.
static ENGLISH_FUNCTION_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "am", "to", "of", "in", "on", "at", "for", "with",
    "from", "by", "about", "and", "but", "or", "so", "if", "then", "than", "because", "as", "i", "you", "he", "she",
    "it", "we", "they", "me", "him", "her", "us", "them", "my", "your", "his", "its", "our", "their", "this", "that",
    "these", "those", "will", "would", "can", "could", "should", "shall", "may", "might", "must", "have", "has", "had",
    "do", "does", "did", "not", "there", "what", "which", "who", "when", "where", "why", "how",
];

/// Hindi and Marathi grammatical function words: copulas, auxiliaries,
/// postpositions, pronouns and light verbs.
static INDIC_FUNCTION_WORDS: &[&str] = &[
    // Hindi
    "hai", "hain", "hu", "hoon", "hun", "ho", "tha", "thi", "hoga", "hogi", "honge", "hota", "hoti", "hote", "hua",
    "hui", "raha", "rahi", "rahe", "ka", "ke", "ki", "ko", "se", "mein", "me", "pe", "par", "tak", "aur", "lekin",
    "toh", "to", "bhi", "na", "nahi", "nahin", "nhi", "kya", "kyun", "kaise", "kahan", "kab", "kaun", "main", "mai",
    "mujhe", "mera", "meri", "mere", "tu", "tum", "tumhe", "aap", "aapko", "apna", "hum", "hume", "woh", "wo", "yeh",
    "ye", "uska", "unka", "iska", "kar", "karo", "karna", "karne", "kiya", "do", "de", "dena", "diya", "lo", "lena",
    "liya", "gaya", "gayi", "jao", "ja", "chahiye", "sakta", "sakte", "wala", "wali", "wale", "agar", "kyunki",
    "isliye", "ji", "haan", // Marathi
    "aahe", "ahe", "aahes", "ahes", "aahet", "ahet", "aahot", "nahiye", "nahis", "nahit", "la", "cha", "chi", "che",
    "chya", "madhe", "madhye", "var", "sathi", "ani", "aani", "pan", "mhanje", "mala", "tula", "tyala", "tila", "amhi",
    "tumhi", "mi", "ti", "te", "majha", "maza", "tujha", "karaycha", "karaychi", "karto", "karte", "kela", "keli",
    "kele", "karun", "karuya", "jhala", "zala", "jhali", "gela", "geli", "ala", "aala", "hoto", "asel", "nako", "kay",
    "kasa", "kashi", "kuthe", "kevha", "ata", "jato", "jate", "yeto", "ghe", "dya",
];

fn detect_latin(sample: &str, script: Script) -> LanguageProfile {
    let dictionary = EnglishDictionary::global();
    let (mut english, mut hindi, mut marathi) = (0.0f32, 0.0f32, 0.0f32);
    let (mut english_function, mut indic_function) = (0.0f32, 0.0f32);
    let mut strong_indic = 0u32;
    let mut unknown = 0.0f32;
    let mut common_english = 0u32;
    let mut token_count = 0u32;

    for token in tokenize(sample) {
        if token.text.chars().any(|c| c.is_ascii_digit()) || token_in_technical_span(sample, &token) {
            continue;
        }
        let lower = token.text.to_lowercase();
        token_count += 1;
        let h = lexicon::hindi_weight(&lower);
        let m = lexicon::marathi_weight(&lower);
        let e = dictionary.english_weight(&lower);
        let w = h.max(m);
        if ENGLISH_FUNCTION_WORDS.contains(&lower.as_str()) {
            english_function += 1.0 - w;
        }
        if INDIC_FUNCTION_WORDS.contains(&lower.as_str()) {
            indic_function += w;
        }
        if e >= 0.85 && w < 0.5 {
            common_english += 1;
        }
        if h == 0.0 && m == 0.0 {
            if e > 0.0 {
                english += e.max(0.5);
            } else {
                unknown += 1.0;
            }
            continue;
        }
        if w >= 0.9 {
            strong_indic += 1;
        }
        english += e * (1.0 - w);
        hindi += w * h / (h + m);
        marathi += w * m / (h + m);
    }

    let indic_known = hindi + marathi;
    let known_total = english + indic_known;
    if unknown > 0.0 {
        if indic_known >= 1.0 && indic_known / known_total.max(f32::EPSILON) >= 0.2 {
            // Unlisted words in visibly Indic text are most likely transliterations.
            let hindi_part = hindi / indic_known;
            hindi += unknown * 0.8 * hindi_part;
            marathi += unknown * 0.8 * (1.0 - hindi_part);
        } else if token_count < 3 || common_english as f32 / token_count as f32 >= 0.35 {
            // Unknown words among common English words are typos or names.
            english += unknown * 0.5;
        } else {
            return LanguageProfile {
                label: LanguageLabel::Other,
                script,
                confidence: 0.4,
                token_count,
                ..LanguageProfile::unknown()
            };
        }
    }

    let total = english + hindi + marathi;
    if total < 0.5 {
        return LanguageProfile { token_count, script, ..LanguageProfile::unknown() };
    }
    let english_share = english / total;
    let hindi_share = hindi / total;
    let marathi_share = marathi / total;
    let indic_share = hindi_share + marathi_share;
    let is_marathi = marathi > hindi;

    // A single distinctive word ("yaar", "aahe") is enough to mark code-mixing;
    // weak, English-looking matches ("to", "main") are not.
    let is_english = indic_share < INDIC_TRACE_SHARE || (indic_share < INDIC_MIN_SHARE && strong_indic == 0);
    let indic_matrix = if english_function + indic_function >= 0.5 {
        indic_function > english_function
    } else {
        indic_share >= INDIC_DOMINANT_SHARE
    };
    let label = match (is_english, indic_matrix, is_marathi) {
        (true, _, _) => LanguageLabel::English,
        (false, true, false) if english_share < PURE_MAX_ENGLISH_SHARE => LanguageLabel::HindiLatin,
        (false, true, true) if english_share < PURE_MAX_ENGLISH_SHARE => LanguageLabel::MarathiLatin,
        (false, true, false) => LanguageLabel::Hinglish,
        (false, true, true) => LanguageLabel::MarathiEnglish,
        (false, false, false) => LanguageLabel::MixedEnglishHindi,
        (false, false, true) => LanguageLabel::MixedEnglishMarathi,
    };

    // Confidence: more tokens and a clearer grammatical frame.
    let frame_total = english_function + indic_function;
    let frame_margin = if frame_total > 0.0 { (english_function - indic_function).abs() / frame_total } else { 0.5 };
    let volume = (token_count as f32 / 6.0).min(1.0);
    let confidence = volume * (0.5 + 0.5 * frame_margin);
    LanguageProfile { label, script, english_share, hindi_share, marathi_share, confidence, token_count }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(text: &str) -> LanguageLabel {
        detect(text).label
    }

    #[test]
    fn english_text() {
        assert_eq!(label("I wanted to inform you that the testing is completed."), LanguageLabel::English);
        assert_eq!(label("The deployment failed because"), LanguageLabel::English);
        assert_eq!(label("I am going to the main office, is that ok?"), LanguageLabel::English);
        assert_eq!(
            label("I wanted to inform you that the testing is completd."),
            LanguageLabel::English,
            "typos stay English"
        );
    }

    #[test]
    fn hindi_transliteration() {
        assert_eq!(label("bhai kal 10 baje milte hai"), LanguageLabel::HindiLatin);
        assert_eq!(label("mujhe nahi pata ki woh kab aayega"), LanguageLabel::HindiLatin);
    }

    #[test]
    fn hinglish_code_mixing() {
        let profile = detect("Please check the build bhai, kal tak fix kar dena");
        assert!(matches!(profile.label, LanguageLabel::Hinglish | LanguageLabel::MixedEnglishHindi), "{profile:?}");
        assert_eq!(label("bhai meeting ka time change kar do please"), LanguageLabel::Hinglish);
    }

    #[test]
    fn english_with_a_few_hindi_words() {
        assert_eq!(
            label("Thanks for the update yaar, I will review the pull request and share my comments today"),
            LanguageLabel::MixedEnglishHindi
        );
    }

    #[test]
    fn marathi_transliteration() {
        let p = detect("udya client la call karaycha aahe");
        assert!(matches!(p.label, LanguageLabel::MarathiEnglish | LanguageLabel::MarathiLatin), "{p:?}");
        let p = detect("mala ha issue samjat nahiye");
        assert!(matches!(p.label, LanguageLabel::MarathiLatin | LanguageLabel::MarathiEnglish), "{p:?}");
        let p = detect("kal deployment karaycha aahe");
        assert!(matches!(p.label, LanguageLabel::MarathiEnglish | LanguageLabel::MarathiLatin), "{p:?}");
        assert_eq!(label("tu kuthe aahes, mi ghari jato aahe"), LanguageLabel::MarathiLatin);
    }

    #[test]
    fn english_with_marathi_words() {
        assert_eq!(
            label("The release is ready and the client approved the final design, udya deploy karuya"),
            LanguageLabel::MixedEnglishMarathi
        );
    }

    #[test]
    fn devanagari_scripts() {
        assert_eq!(label("मुझे कल ऑफिस जाना है"), LanguageLabel::HindiDevanagari);
        assert_eq!(label("मला उद्या ऑफिसला जायचं आहे"), LanguageLabel::MarathiDevanagari);
        assert_eq!(detect("मला उद्या ऑफिसला जायचं आहे").script, Script::Devanagari);
    }

    #[test]
    fn other_and_unknown() {
        assert_eq!(label(""), LanguageLabel::Unknown);
        assert_eq!(label("12345 !!!"), LanguageLabel::Unknown);
        assert_eq!(label("こんにちは世界"), LanguageLabel::Other);
    }

    #[test]
    fn urls_and_numbers_are_ignored() {
        assert_eq!(label("see https://example.com/kal/hai for details"), LanguageLabel::English);
    }

    #[test]
    fn preservation_instructions_never_ask_to_translate_indic_text() {
        for l in [
            LanguageLabel::Hinglish,
            LanguageLabel::MarathiLatin,
            LanguageLabel::HindiLatin,
            LanguageLabel::MarathiEnglish,
        ] {
            assert!(l.preservation_instruction().contains("Do not translate"));
            assert!(l.has_indic());
        }
        assert!(!LanguageLabel::English.has_indic());
    }

    #[test]
    fn confidence_grows_with_text() {
        let short = detect("bhai");
        let long = detect("bhai kal 10 baje milte hai, tum aa rahe ho na");
        assert!(long.confidence > short.confidence);
    }
}
