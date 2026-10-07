//! Embedded English word-frequency dictionary.
//!
//! The base list is SymSpell's `frequency_dictionary_en_82_765.txt` (MIT, see
//! `THIRD_PARTY_NOTICES.md`), extended with technology and chat vocabulary from
//! `data/en_supplement.txt`. It is parsed lazily on first use and kept for the
//! lifetime of the process; entries borrow from the embedded text, so no word
//! strings are copied.

use std::collections::HashMap;
use std::sync::OnceLock;

static FREQUENCY_LIST: &str = include_str!("../../data/en_frequency.txt");
static SUPPLEMENT_LIST: &str = include_str!("../../data/en_supplement.txt");

/// Frequency assigned to supplement words: comparable to a common word, so they
/// are never flagged and count as English during language detection.
const SUPPLEMENT_FREQUENCY: u64 = 8_000_000;

/// A dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Corpus frequency count.
    pub frequency: u64,
    /// 1-based frequency rank (1 = most frequent).
    pub rank: u32,
}

/// Lowercase English words with corpus frequencies.
#[derive(Debug)]
pub struct EnglishDictionary {
    words: HashMap<&'static str, Entry>,
}

impl EnglishDictionary {
    /// The process-wide dictionary instance.
    pub fn global() -> &'static EnglishDictionary {
        static DICTIONARY: OnceLock<EnglishDictionary> = OnceLock::new();
        DICTIONARY.get_or_init(Self::load)
    }

    fn load() -> Self {
        let mut words: HashMap<&'static str, Entry> = HashMap::with_capacity(84_000);
        let mut rank = 0u32;
        for line in FREQUENCY_LIST.lines() {
            let mut parts = line.split_whitespace();
            let (Some(word), Some(freq)) = (parts.next(), parts.next()) else {
                continue;
            };
            let Ok(frequency) = freq.parse::<u64>() else {
                continue;
            };
            rank += 1;
            words.entry(word).or_insert(Entry { frequency, rank });
        }
        // Supplement words slot in at the rank a word of SUPPLEMENT_FREQUENCY would hold.
        let supplement_rank = words.values().filter(|e| e.frequency >= SUPPLEMENT_FREQUENCY).count() as u32 + 1;
        for line in SUPPLEMENT_LIST.lines() {
            let word = line.trim();
            if word.is_empty() || word.starts_with('#') {
                continue;
            }
            words.entry(word).or_insert(Entry { frequency: SUPPLEMENT_FREQUENCY, rank: supplement_rank });
        }
        Self { words }
    }

    /// Looks up a lowercase word.
    pub fn get(&self, lowercase_word: &str) -> Option<Entry> {
        self.words.get(lowercase_word).copied()
    }

    /// Whether the lowercase word is known.
    pub fn contains(&self, lowercase_word: &str) -> bool {
        self.words.contains_key(lowercase_word)
    }

    /// Number of words in the dictionary.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Whether the dictionary is empty (never true for the embedded list).
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// How confidently `lowercase_word` reads as English, from 0.0 (unknown) to
    /// 1.0 (very common), based on its frequency rank.
    pub fn english_weight(&self, lowercase_word: &str) -> f32 {
        match self.get(lowercase_word) {
            None => 0.0,
            Some(e) if e.rank <= 3_000 => 1.0,
            Some(e) if e.rank <= 12_000 => 0.85,
            Some(e) if e.rank <= 30_000 => 0.6,
            Some(_) => 0.35,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_base_and_supplement() {
        let d = EnglishDictionary::global();
        assert!(d.len() > 80_000);
        assert_eq!(d.get("the").map(|e| e.rank), Some(1));
        assert!(d.contains("deployment"));
        assert!(d.contains("kubernetes"), "supplement word");
        assert!(d.contains("ok"), "supplement word");
        assert!(!d.contains("completd"));
    }

    #[test]
    fn english_weight_reflects_frequency() {
        let d = EnglishDictionary::global();
        assert_eq!(d.english_weight("the"), 1.0);
        assert!(d.english_weight("deployment") >= 0.85);
        assert_eq!(d.english_weight("karaycha"), 0.0);
    }
}
