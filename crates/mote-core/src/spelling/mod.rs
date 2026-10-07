//! Local, deterministic English spelling assistance.
//!
//! Candidates are generated with single and double edits (Norvig-style) and
//! ranked by corpus frequency combined with a simple typing-error model: a
//! missing letter or swapped pair is a far more common typo than an unrelated
//! substitution, so `completd` becomes `completed` rather than the more frequent
//! `complete`.
//!
//! The checker never touches romanized Hindi/Marathi words, proper nouns, code,
//! URLs or anything in an Indic-dominant text: Mote preserves the user's
//! language and only corrects what is clearly an English typo.

pub mod dictionary;

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::language::{lexicon, LanguageProfile};
use crate::text::{is_sentence_terminator, token_in_technical_span, tokenize, Token};
use dictionary::EnglishDictionary;

/// A misspelled word and its proposed correction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Misspelling {
    /// Byte offset of the word within the checked text.
    pub start: usize,
    /// Byte offset one past the end of the word.
    pub end: usize,
    pub word: String,
    pub suggestion: String,
    pub confidence: f32,
}

/// A ranked correction for a single word.
#[derive(Debug, Clone, PartialEq)]
pub struct Correction {
    pub word: String,
    pub confidence: f32,
}

/// Minimum confidence for a correction to be offered.
pub const DEFAULT_MIN_CONFIDENCE: f32 = 0.6;
/// Short words have many plausible neighbours, so they need more certainty.
const SHORT_WORD_MIN_CONFIDENCE: f32 = 0.85;
/// Corrections must be reasonably common words.
const MIN_CANDIDATE_FREQUENCY: u64 = 50_000;
const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

/// Checks `text` and returns likely English misspellings with corrections.
///
/// `ignored` holds lowercase words the user asked Mote to accept.
pub fn check(text: &str, profile: &LanguageProfile, ignored: &HashSet<String>) -> Vec<Misspelling> {
    if profile.label.indic_dominant() || matches!(profile.label, crate::language::LanguageLabel::Other) {
        return Vec::new();
    }
    let dictionary = EnglishDictionary::global();
    let tokens = tokenize(text);
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    for t in &tokens {
        *occurrences.entry(t.text.to_lowercase()).or_default() += 1;
    }

    let mut found = Vec::new();
    for token in &tokens {
        if !is_checkable(text, token) {
            continue;
        }
        let lower = token.text.to_lowercase();
        if dictionary.contains(&lower) || lexicon::is_indic_word(&lower) || ignored.contains(&lower) {
            continue;
        }
        // A word the user repeats is probably intentional jargon or a name.
        if occurrences.get(&lower).copied().unwrap_or(0) > 1 {
            continue;
        }
        let Some(correction) = suggest(&lower) else { continue };
        let threshold = if lower.len() <= 4 { SHORT_WORD_MIN_CONFIDENCE } else { DEFAULT_MIN_CONFIDENCE };
        if correction.confidence < threshold {
            continue;
        }
        found.push(Misspelling {
            start: token.start,
            end: token.end,
            word: token.text.to_string(),
            suggestion: match_case(token.text, &correction.word),
            confidence: correction.confidence,
        });
    }
    found
}

/// Applies all corrections to `text`, returning the corrected string.
pub fn apply(text: &str, misspellings: &[Misspelling]) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut cursor = 0;
    for m in misspellings {
        if m.start < cursor || m.end > text.len() {
            continue;
        }
        out.push_str(&text[cursor..m.start]);
        out.push_str(&m.suggestion);
        cursor = m.end;
    }
    out.push_str(&text[cursor..]);
    out
}

fn is_checkable(text: &str, token: &Token<'_>) -> bool {
    let word = token.text;
    let len = word.chars().count();
    if !(3..=20).contains(&len) || !word.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    // Acronyms, camelCase and mid-sentence capitalised names are left alone.
    if word.chars().skip(1).any(|c| c.is_ascii_uppercase()) {
        return false;
    }
    let capitalised = word.starts_with(|c: char| c.is_ascii_uppercase());
    if capitalised && !at_sentence_start(text, token.start) {
        return false;
    }
    !token_in_technical_span(text, token)
}

fn at_sentence_start(text: &str, start: usize) -> bool {
    let before = text[..start].trim_end();
    before.is_empty() || before.ends_with(is_sentence_terminator)
}

fn match_case(original: &str, correction: &str) -> String {
    if original.starts_with(|c: char| c.is_ascii_uppercase()) {
        let mut chars = correction.chars();
        match chars.next() {
            Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
            None => String::new(),
        }
    } else {
        correction.to_string()
    }
}

/// Kind of edit that turns the typed word into a candidate.
#[derive(Debug, Clone, Copy)]
enum Edit {
    /// The typed word is missing a letter.
    Insert { doubled: bool },
    /// Two adjacent letters were swapped.
    Transpose,
    /// One letter was mistyped.
    Replace { adjacent_key: bool, vowel_swap: bool },
    /// An extra letter was typed.
    Delete { doubled: bool },
}

impl Edit {
    /// Log-scale bonus approximating how common this typo is.
    fn bonus(self) -> f64 {
        match self {
            Edit::Insert { doubled: true } => 3.0,
            Edit::Insert { doubled: false } => 2.5,
            Edit::Transpose => 2.0,
            Edit::Replace { adjacent_key: true, .. } => 1.0,
            Edit::Replace { vowel_swap: true, .. } => 0.8,
            Edit::Replace { .. } => -0.5,
            Edit::Delete { doubled: true } => 1.5,
            Edit::Delete { doubled: false } => 0.5,
        }
    }
}

/// Suggests the most likely correction for a lowercase word.
pub fn suggest(lower: &str) -> Option<Correction> {
    let dictionary = EnglishDictionary::global();
    let mut scored: HashMap<String, f64> = HashMap::new();
    for (candidate, edit) in edits1(lower) {
        if let Some(entry) = dictionary.get(&candidate).filter(|e| e.frequency >= MIN_CANDIDATE_FREQUENCY) {
            let score = (entry.frequency as f64).ln() + edit.bonus();
            let slot = scored.entry(candidate).or_insert(f64::MIN);
            *slot = slot.max(score);
        }
    }
    if scored.is_empty() && (5..=14).contains(&lower.len()) {
        for (first, edit_a) in edits1(lower) {
            for (candidate, edit_b) in edits1(&first) {
                if let Some(entry) = dictionary.get(&candidate).filter(|e| e.frequency >= MIN_CANDIDATE_FREQUENCY) {
                    let score = (entry.frequency as f64).ln() + edit_a.bonus() + edit_b.bonus() - 4.0;
                    let slot = scored.entry(candidate).or_insert(f64::MIN);
                    *slot = slot.max(score);
                }
            }
        }
    }
    scored.remove(lower);
    let mut ranked: Vec<(String, f64)> = scored.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let (best, best_score) = ranked.first()?.clone();
    let runner_up = ranked.get(1).map_or(best_score - 3.0, |(_, s)| *s);
    let confidence = 1.0 / (1.0 + (-(best_score - runner_up) * 2.0).exp());
    Some(Correction { word: best, confidence: confidence as f32 })
}

fn is_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u')
}

fn edits1(word: &str) -> Vec<(String, Edit)> {
    let bytes = word.as_bytes();
    let n = bytes.len();
    let mut out = Vec::with_capacity(54 * n + 25);
    for i in 0..n {
        // Deletion: the user typed bytes[i] by mistake.
        let doubled = (i > 0 && bytes[i - 1] == bytes[i]) || (i + 1 < n && bytes[i + 1] == bytes[i]);
        let mut candidate = Vec::with_capacity(n);
        candidate.extend_from_slice(&bytes[..i]);
        candidate.extend_from_slice(&bytes[i + 1..]);
        out.push((candidate, Edit::Delete { doubled }));
        // Transposition of bytes[i] and bytes[i + 1].
        if i + 1 < n && bytes[i] != bytes[i + 1] {
            let mut candidate = bytes.to_vec();
            candidate.swap(i, i + 1);
            out.push((candidate, Edit::Transpose));
        }
        // Replacement of bytes[i].
        for &c in ALPHABET {
            if c != bytes[i] {
                let mut candidate = bytes.to_vec();
                candidate[i] = c;
                let edit = Edit::Replace {
                    adjacent_key: keys_adjacent(bytes[i], c),
                    vowel_swap: is_vowel(bytes[i]) && is_vowel(c),
                };
                out.push((candidate, edit));
            }
        }
    }
    // Insertion: the user skipped a letter.
    for i in 0..=n {
        for &c in ALPHABET {
            let doubled = (i > 0 && bytes[i - 1] == c) || (i < n && bytes[i] == c);
            let mut candidate = Vec::with_capacity(n + 1);
            candidate.extend_from_slice(&bytes[..i]);
            candidate.push(c);
            candidate.extend_from_slice(&bytes[i..]);
            out.push((candidate, Edit::Insert { doubled }));
        }
    }
    out.into_iter().filter_map(|(bytes, edit)| String::from_utf8(bytes).ok().map(|s| (s, edit))).collect()
}

/// Whether two lowercase letters are neighbours on a QWERTY keyboard.
fn keys_adjacent(a: u8, b: u8) -> bool {
    const ROWS: [&[u8]; 3] = [b"qwertyuiop", b"asdfghjkl", b"zxcvbnm"];
    let position = |c: u8| {
        ROWS.iter()
            .enumerate()
            .find_map(|(row, keys)| keys.iter().position(|k| *k == c).map(|col| (row as i32, col as i32)))
    };
    match (position(a), position(b)) {
        (Some((ra, ca)), Some((rb, cb))) => {
            // Rows are staggered: row r+1 is shifted right by about half a key.
            let dr = rb - ra;
            let dc = cb - ca;
            match dr {
                0 => dc.abs() == 1,
                1 => dc == 0 || dc == -1,
                -1 => dc == 0 || dc == 1,
                _ => false,
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::detect;

    fn corrections(text: &str) -> Vec<(String, String)> {
        let profile = detect(text);
        check(text, &profile, &HashSet::new()).into_iter().map(|m| (m.word, m.suggestion)).collect()
    }

    #[test]
    fn spec_example_completd() {
        let text = "I wanted to inform you that the testing is completd.";
        assert_eq!(corrections(text), vec![("completd".to_string(), "completed".to_string())]);
        let profile = detect(text);
        let found = check(text, &profile, &HashSet::new());
        assert_eq!(apply(text, &found), "I wanted to inform you that the testing is completed.");
    }

    #[test]
    fn common_typos() {
        for (typo, fixed) in [
            ("teh", "the"),
            ("recieve", "receive"),
            ("definately", "definitely"),
            ("occured", "occurred"),
            ("untill", "until"),
            ("wierd", "weird"),
            ("becuase", "because"),
            ("goverment", "government"),
            ("adress", "address"),
            ("thier", "their"),
            ("tommorow", "tomorrow"),
        ] {
            assert_eq!(suggest(typo).map(|c| c.word), Some(fixed.to_string()), "{typo}");
        }
    }

    #[test]
    fn preserves_case_at_sentence_start() {
        assert_eq!(corrections("Teh build passed."), vec![("Teh".to_string(), "The".to_string())]);
    }

    #[test]
    fn leaves_correct_text_alone() {
        assert!(corrections("The deployment failed because the Redis container was unavailable.").is_empty());
    }

    #[test]
    fn never_corrects_transliterated_text() {
        assert!(corrections("bhai kal 10 baje milte hai").is_empty());
        assert!(corrections("udya client la call karaycha aahe").is_empty());
        assert!(corrections("mala ha issue samjat nahiye").is_empty());
        assert!(corrections("kal deployment karaycha aahe").is_empty());
    }

    #[test]
    fn mixed_text_only_corrects_english_typos() {
        let found = corrections("Thanks yaar, the testing is completd and I will deploy kal");
        assert_eq!(found, vec![("completd".to_string(), "completed".to_string())]);
    }

    #[test]
    fn skips_names_code_urls_and_acronyms() {
        assert!(corrections("I met Priyanka at the office").is_empty());
        assert!(corrections("Check src/mian.rs and https://exmaple.com now").is_empty());
        assert!(corrections("The NASAA report and myVarr value").is_empty());
    }

    #[test]
    fn repeated_unknown_words_are_treated_as_intentional() {
        assert!(corrections("The flarbex service talks to another flarbex instance").is_empty());
    }

    #[test]
    fn respects_ignore_list() {
        let text = "Ship the completd build";
        let mut ignored = HashSet::new();
        ignored.insert("completd".to_string());
        assert!(check(text, &detect(text), &ignored).is_empty());
    }

    #[test]
    fn keyboard_adjacency() {
        assert!(keys_adjacent(b'd', b'e'));
        assert!(keys_adjacent(b'a', b's'));
        assert!(keys_adjacent(b'g', b'b'));
        assert!(!keys_adjacent(b'a', b'p'));
    }
}
