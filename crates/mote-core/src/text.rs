//! Text utilities shared by the language, spelling, intent and completion modules.
//!
//! Accessibility APIs on both macOS and Windows index text in UTF-16 code units,
//! while Rust strings are UTF-8. The helpers here convert between the two and
//! provide the tokenization used throughout the core.

use unicode_segmentation::UnicodeSegmentation;

/// Number of UTF-16 code units in `s`.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Converts a UTF-16 offset into a byte offset into `s`.
///
/// Returns `None` when the offset lies past the end of the string or falls
/// inside a surrogate pair.
pub fn utf16_to_byte_offset(s: &str, utf16_offset: usize) -> Option<usize> {
    let mut units = 0usize;
    for (byte_idx, ch) in s.char_indices() {
        if units == utf16_offset {
            return Some(byte_idx);
        }
        units += ch.len_utf16();
        if units > utf16_offset {
            return None;
        }
    }
    (units == utf16_offset).then_some(s.len())
}

/// Returns the suffix of `s` that contains at most `max_chars` characters.
pub fn tail_chars(s: &str, max_chars: usize) -> &str {
    let count = s.chars().count();
    if count <= max_chars {
        return s;
    }
    let skip = count - max_chars;
    let start = s.char_indices().nth(skip).map_or(s.len(), |(i, _)| i);
    &s[start..]
}

/// Like [`tail_chars`], but advances the cut to the next word boundary so the
/// returned slice does not begin with a partial word (when a boundary exists).
pub fn tail_at_word_boundary(s: &str, max_chars: usize) -> &str {
    let tail = tail_chars(s, max_chars);
    if tail.len() == s.len() {
        return tail;
    }
    let cut = s.len() - tail.len();
    let on_boundary = s[..cut].chars().last().is_some_and(char::is_whitespace) || tail.starts_with(char::is_whitespace);
    if on_boundary {
        return tail.trim_start();
    }
    match tail.find(char::is_whitespace) {
        Some(ws) => {
            let rest = tail[ws..].trim_start();
            if rest.is_empty() {
                tail
            } else {
                rest
            }
        }
        None => tail,
    }
}

/// Returns the prefix of `s` containing at most `max_chars` characters.
pub fn head_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// A word token and its byte span within the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub text: &'a str,
    pub start: usize,
    pub end: usize,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

fn is_joiner(c: char) -> bool {
    matches!(c, '\'' | '’' | '-')
}

/// Splits text into word tokens.
///
/// A token is a run of alphanumeric characters; apostrophes and hyphens are kept
/// when they join two word characters (`don't`, `follow-up`).
pub fn tokenize(s: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        if !is_word_char(c) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() {
            let (_, cj) = chars[j];
            if is_word_char(cj) {
                j += 1;
            } else if is_joiner(cj) && j + 1 < chars.len() && is_word_char(chars[j + 1].1) {
                j += 2;
            } else {
                break;
            }
        }
        let end = chars.get(j).map_or(s.len(), |(b, _)| *b);
        tokens.push(Token { text: &s[start..end], start, end });
        i = j;
    }
    tokens
}

/// Number of word tokens in `s`.
pub fn word_count(s: &str) -> usize {
    tokenize(s).len()
}

/// Number of user-perceived characters (grapheme clusters) in `s`.
///
/// This is the number of Backspace presses needed to delete `s` in most editors.
pub fn grapheme_count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// Whether `c` terminates a sentence.
pub fn is_sentence_terminator(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '\n' | '।' | '॥')
}

/// Byte offset at which the last sentence of `s` begins (after the previous
/// terminator and any following whitespace). Trailing terminators of the final
/// sentence are part of that sentence.
pub fn last_sentence_start(s: &str) -> usize {
    let trimmed_end = s.trim_end_matches(|c: char| is_sentence_terminator(c) || c.is_whitespace());
    let boundary =
        trimmed_end.char_indices().rev().find(|(_, c)| is_sentence_terminator(*c)).map_or(0, |(i, c)| i + c.len_utf8());
    let rest = &s[boundary..];
    boundary + (rest.len() - rest.trim_start().len())
}

/// Whether the text ends inside a word (no trailing whitespace or punctuation).
pub fn ends_mid_word(s: &str) -> bool {
    s.chars().last().is_some_and(is_word_char)
}

/// The last word token of `s`, if any.
pub fn last_token(s: &str) -> Option<Token<'_>> {
    tokenize(s).pop()
}

/// Collapses runs of whitespace into single spaces and trims the ends.
pub fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 64-bit FNV-1a hash. Not cryptographic: used only for in-memory dedupe keys,
/// never persisted.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Whether the token looks like part of a URL, email address, file path, mention
/// or hashtag in `text`, judged by the characters around it.
pub fn token_in_technical_span(text: &str, token: &Token<'_>) -> bool {
    let span_start = text[..token.start].rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let span_end = text[token.end..].find(char::is_whitespace).map_or(text.len(), |i| token.end + i);
    let span = &text[span_start..span_end];
    let chars: Vec<char> = span.chars().collect();
    let internal_dot = chars.windows(2).any(|w| w[0] == '.' && w[1].is_alphanumeric());
    span.contains("://")
        || span.contains('@')
        || span.contains('/')
        || span.contains('\\')
        || span.contains('_')
        || span.starts_with('#')
        || span.contains('=')
        || span.contains('`')
        || internal_dot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_handle_multibyte_text() {
        let s = "a😀b é";
        assert_eq!(utf16_len(s), 6); // a(1) 😀(2) b(1) space(1) é(1)
        assert_eq!(utf16_to_byte_offset(s, 0), Some(0));
        assert_eq!(utf16_to_byte_offset(s, 1), Some(1));
        assert_eq!(utf16_to_byte_offset(s, 2), None, "inside surrogate pair");
        assert_eq!(utf16_to_byte_offset(s, 3), Some(5));
        assert_eq!(utf16_to_byte_offset(s, 6), Some(s.len()));
        assert_eq!(utf16_to_byte_offset(s, 7), None);
    }

    #[test]
    fn devanagari_offsets() {
        let s = "मला काम आहे";
        let len = utf16_len(s);
        assert_eq!(utf16_to_byte_offset(s, len), Some(s.len()));
    }

    #[test]
    fn tail_and_head() {
        assert_eq!(tail_chars("hello world", 5), "world");
        assert_eq!(tail_chars("hi", 5), "hi");
        assert_eq!(head_chars("hello", 2), "he");
        assert_eq!(tail_at_word_boundary("the quick brown fox", 12), "brown fox");
        assert_eq!(tail_at_word_boundary("short", 12), "short");
    }

    #[test]
    fn tokenization_keeps_contractions_and_hyphens() {
        let toks: Vec<&str> = tokenize("Don't stop—follow-up at 10 baje, ok?").iter().map(|t| t.text).collect();
        assert_eq!(toks, vec!["Don't", "stop", "follow-up", "at", "10", "baje", "ok"]);
    }

    #[test]
    fn tokenization_spans_are_exact() {
        let s = "  ab  cd";
        let toks = tokenize(s);
        assert_eq!(toks[0], Token { text: "ab", start: 2, end: 4 });
        assert_eq!(&s[toks[1].start..toks[1].end], "cd");
    }

    #[test]
    fn sentence_boundaries() {
        let s = "First one. Second is here";
        assert_eq!(&s[last_sentence_start(s)..], "Second is here");
        let s = "Done. The testing is completd.";
        assert_eq!(&s[last_sentence_start(s)..], "The testing is completd.");
        let s = "no terminator at all";
        assert_eq!(last_sentence_start(s), 0);
        let s = "Line one\nline two";
        assert_eq!(&s[last_sentence_start(s)..], "line two");
    }

    #[test]
    fn grapheme_counting() {
        assert_eq!(grapheme_count("é"), 1);
        assert_eq!(grapheme_count("👍🏽"), 1);
        assert_eq!(grapheme_count("abc."), 4);
    }

    #[test]
    fn mid_word_detection() {
        assert!(ends_mid_word("deploym"));
        assert!(!ends_mid_word("deploy "));
        assert!(!ends_mid_word("done."));
        assert!(!ends_mid_word(""));
    }

    #[test]
    fn technical_spans() {
        let text = "see https://example.com/path and user@example.com or src/main.rs";
        for tok in tokenize(text) {
            let technical = token_in_technical_span(text, &tok);
            match tok.text {
                "see" | "and" | "or" => assert!(!technical, "{}", tok.text),
                "example" | "com" | "path" | "user" | "src" | "main" | "rs" => assert!(technical, "{}", tok.text),
                _ => {}
            }
        }
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_ne!(fnv1a64(b"a"), fnv1a64(b"b"));
    }
}
