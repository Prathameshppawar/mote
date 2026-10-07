//! Text from other applications can contain any Unicode. None of Mote's text
//! processing may panic on it: a panic would stop the assistance engine.

use std::collections::HashSet;

use mote_core::assistance::{correction_from_ai, grammar_candidate, grammar_risk, spelling_correction};
use mote_core::completion::clean_completion;
use mote_core::context::clipboard;
use mote_core::intent::apps::AppCategory;
use mote_core::intent::{classify, IntentSignals, IntentSubtype};
use mote_core::language::detect;
use mote_core::platform::InputRole;
use mote_core::spelling;
use mote_core::text::{
    last_sentence_start, last_token, normalize_whitespace, tail_at_word_boundary, tail_chars, token_in_technical_span,
    tokenize,
};

/// Whitespace and joiners that are more than one byte in UTF-8.
const SPACES: &[&str] = &["\u{a0}", "\u{202f}", "\u{2009}", "\u{3000}", "\u{2028}", "\u{200b}", "\u{feff}", "\t", "\n"];

const FRAGMENTS: &[&str] = &[
    "a",
    "teh",
    "recieve",
    "Hello",
    "bhai",
    "kar do",
    "नमस्ते",
    "मराठी",
    "कृपया",
    "😀",
    "👩‍💻",
    "e\u{301}",
    "https://example.com/a?b=c",
    "user@example.com",
    "C:\\path\\file.txt",
    "#tag",
    "don't",
    "«quote»",
    ".",
    "?",
    "।",
    "...",
];

fn samples() -> Vec<String> {
    let mut out = Vec::new();
    for space in SPACES {
        for a in FRAGMENTS {
            for b in FRAGMENTS {
                out.push(format!("{a}{space}{b}"));
                out.push(format!("{space}{a}{space}{b}{space}"));
                out.push(format!("I {a}{space}{b} the report."));
            }
        }
    }
    out.push(String::new());
    out.push("\u{a0}".repeat(50));
    out.push(FRAGMENTS.join("\u{a0}"));
    out.push(FRAGMENTS.join("\u{3000}"));
    out
}

#[test]
fn text_processing_never_panics_on_unusual_unicode() {
    let ignored = HashSet::new();
    let checked = HashSet::new();
    for text in samples() {
        let language = detect(&text);
        let _ = spelling::check(&text, &language, &ignored);
        let _ = spelling_correction(&text, &language, &ignored);
        let _ = grammar_candidate(&text, &language, Some(IntentSubtype::Email), &checked);
        let _ = grammar_risk(&text, Some(IntentSubtype::Chat));
        let _ = correction_from_ai(&text, &format!("{text} fixed"));
        let _ = clean_completion(&text, "The report", 12, true);
        let _ = clean_completion(" and then", &text, 12, false);
        let _ = clipboard::classify(&text);
        let _ = last_sentence_start(&text);
        let _ = last_token(&text);
        let _ = normalize_whitespace(&text);
        for n in [0, 1, 2, 3, 5, 8] {
            let _ = tail_chars(&text, n);
            let _ = tail_at_word_boundary(&text, n);
        }
        for token in tokenize(&text) {
            let _ = token_in_technical_span(&text, &token);
        }
        let signals = IntentSignals {
            category: AppCategory::Browser,
            role: Some(InputRole::TextArea),
            is_multiline: true,
            placeholder: Some(&text),
            label: None,
            text: &text,
            language: &language,
            clipboard_kind: None,
            previous_category: None,
            user_override: None,
        };
        let _ = classify(&signals);
    }
}

#[test]
fn a_word_after_a_non_breaking_space_is_handled() {
    let text = "a\u{a0}b";
    let tokens = tokenize(text);
    assert!(!tokens.is_empty());
    for token in &tokens {
        assert!(!token_in_technical_span(text, token));
    }
    let language = detect("I will recieve\u{a0}teh files");
    let _ = spelling::check("I will recieve\u{a0}teh files", &language, &HashSet::new());
}
