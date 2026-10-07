//! Writing assistance decisions and text edits.
//!
//! Deterministic signals come first: local spelling suggestions cost no tokens,
//! and a cheap "grammar risk" heuristic decides whether a sentence is worth an
//! AI grammar check at all. Accepted suggestions become [`EditPlan`]s that the
//! platform layer applies to the focused application.

use std::collections::HashSet;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::intent::{IntentKind, IntentSubtype};
use crate::language::{LanguageLabel, LanguageProfile};
use crate::platform::{Key, PlatformAdapter, PlatformError, ReadLimits};
use crate::spelling;
use crate::text::{fnv1a64, grapheme_count, is_sentence_terminator, last_sentence_start, tokenize, word_count};

/// A correction of the text just before the caret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Correction {
    /// The text at the end of the field that will be replaced.
    pub original_tail: String,
    /// Its replacement.
    pub corrected_tail: String,
    /// What to show: the changed word(s).
    pub display_from: String,
    pub display_to: String,
    /// Number of separate changes.
    pub changes: u32,
    pub source: CorrectionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CorrectionSource {
    LocalSpelling,
    AiGrammar,
}

/// Whether writing assistance applies to this context.
pub fn writing_applies(kind: IntentKind) -> bool {
    matches!(kind, IntentKind::Conversation | IntentKind::Note | IntentKind::Prompt)
}

/// Local spelling suggestion for the sentence before the caret.
///
/// Words still being typed (the last word, when the text ends mid-word) are
/// never flagged.
pub fn spelling_correction(
    text_before: &str,
    language: &LanguageProfile,
    ignored: &HashSet<String>,
) -> Option<Correction> {
    let start = last_sentence_start(text_before);
    let sentence = &text_before[start..];
    if sentence.trim().is_empty() {
        return None;
    }
    let mut found = spelling::check(sentence, language, ignored);
    if text_before.ends_with(char::is_alphanumeric) {
        let last = tokenize(sentence).last().map(|t| t.start);
        found.retain(|m| Some(m.start) != last);
    }
    let first = found.first()?;
    let tail_start = first.start;
    let original_tail = sentence[tail_start..].to_string();
    let shifted: Vec<spelling::Misspelling> = found
        .iter()
        .map(|m| spelling::Misspelling { start: m.start - tail_start, end: m.end - tail_start, ..m.clone() })
        .collect();
    let corrected_tail = spelling::apply(&original_tail, &shifted);
    Some(Correction {
        display_from: first.word.clone(),
        display_to: first.suggestion.clone(),
        changes: u32::try_from(found.len()).unwrap_or(u32::MAX),
        original_tail,
        corrected_tail,
        source: CorrectionSource::LocalSpelling,
    })
}

const MISSING_APOSTROPHE: &[&str] = &[
    "dont", "cant", "wont", "isnt", "doesnt", "didnt", "im", "ive", "youre", "theyre", "wasnt", "werent", "couldnt",
    "shouldnt", "wouldnt", "hasnt", "havent", "arent", "thats", "whats",
];
const AGREEMENT_ERRORS: &[&str] = &[
    "he don't",
    "she don't",
    "it don't",
    "they was",
    "we was",
    "you was",
    "i is",
    "he have",
    "she have",
    "it have",
    "i has",
    "there is many",
    "people was",
    "could of",
    "should of",
    "would of",
    "your welcome",
    "alot",
];

/// Cheap local check: does this sentence likely contain a grammar mistake?
///
/// Used to decide whether an AI grammar check is worth its tokens.
pub fn grammar_risk(sentence: &str, subtype: Option<IntentSubtype>) -> bool {
    let lower = format!(" {} ", sentence.to_lowercase());
    let tokens: Vec<String> = tokenize(&lower).iter().map(|t| t.text.to_string()).collect();
    // Lowercase pronoun "i" (checked on the original casing).
    let padded = format!(" {sentence} ");
    if padded.contains(" i ") || padded.contains(" i'") || padded.contains(" i,") {
        return true;
    }
    if tokens.iter().any(|t| MISSING_APOSTROPHE.contains(&t.as_str())) {
        return true;
    }
    if AGREEMENT_ERRORS.iter().any(|e| lower.contains(&format!(" {e} "))) {
        return true;
    }
    if tokens.windows(2).any(|w| w[0] == w[1] && w[0].chars().all(char::is_alphabetic) && w[0].len() > 1) {
        return true;
    }
    // "a apple", "an car"
    for w in tokens.windows(2) {
        let next = w[1].chars().next().unwrap_or('x');
        if w[0] == "a" && "aeio".contains(next) && !w[1].starts_with("one") && !w[1].starts_with("eu") {
            return true;
        }
        if w[0] == "an" && next.is_ascii_alphabetic() && !"aeiouh".contains(next) {
            return true;
        }
    }
    // Formal writing should start with a capital letter.
    let formal = matches!(subtype, Some(IntentSubtype::Email | IntentSubtype::ProfessionalMessage));
    formal && sentence.trim_start().starts_with(|c: char| c.is_ascii_lowercase())
}

/// The sentence to send for an AI grammar check, if one is warranted.
pub fn grammar_candidate(
    text_before: &str,
    language: &LanguageProfile,
    subtype: Option<IntentSubtype>,
    already_checked: &HashSet<u64>,
) -> Option<String> {
    if !text_before.trim_end().ends_with(is_sentence_terminator) {
        return None;
    }
    if !matches!(
        language.label,
        LanguageLabel::English | LanguageLabel::MixedEnglishHindi | LanguageLabel::MixedEnglishMarathi
    ) {
        return None;
    }
    let sentence = text_before[last_sentence_start(text_before)..].trim_end();
    let words = word_count(sentence);
    if !(4..=60).contains(&words) || sentence.chars().count() > 400 {
        return None;
    }
    if already_checked.contains(&fnv1a64(sentence.as_bytes())) || !grammar_risk(sentence, subtype) {
        return None;
    }
    Some(sentence.to_string())
}

/// Builds a correction from an AI-corrected sentence, if it changed anything.
pub fn correction_from_ai(original_sentence: &str, corrected: &str) -> Option<Correction> {
    let corrected = corrected.trim();
    let original = original_sentence.trim_end();
    if corrected.is_empty()
        || crate::text::normalize_whitespace(corrected) == crate::text::normalize_whitespace(original)
    {
        return None;
    }
    // Reject rewrites that are not corrections (length changed dramatically).
    let (a, b) = (original.chars().count() as f32, corrected.chars().count() as f32);
    if b > a * 1.5 + 10.0 || b < a * 0.6 {
        return None;
    }
    let (from, to) = first_difference(original, corrected);
    Some(Correction {
        original_tail: original.to_string(),
        corrected_tail: corrected.to_string(),
        display_from: from,
        display_to: to,
        changes: 1,
        source: CorrectionSource::AiGrammar,
    })
}

/// The first differing word span between two sentences, for display.
fn first_difference(a: &str, b: &str) -> (String, String) {
    let ta: Vec<&str> = a.split_whitespace().collect();
    let tb: Vec<&str> = b.split_whitespace().collect();
    let prefix = ta.iter().zip(tb.iter()).take_while(|(x, y)| x == y).count();
    let suffix = ta.iter().rev().zip(tb.iter().rev()).take_while(|(x, y)| x == y).count();
    let a_end = ta.len().saturating_sub(suffix).max(prefix);
    let b_end = tb.len().saturating_sub(suffix).max(prefix);
    (ta[prefix..a_end].join(" "), tb[prefix..b_end].join(" "))
}

/// How an accepted suggestion changes the focused field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditPlan {
    /// Insert at the caret.
    Insert { text: String },
    /// Delete the given text just before the caret, then insert a replacement.
    ReplaceBeforeCaret { expected_tail: String, replacement: String },
    /// Replace the current selection.
    ReplaceSelection { text: String },
    /// Replace the whole field.
    ReplaceAll { text: String },
}

/// Records clipboard writes Mote makes so the observer can ignore them.
pub trait ClipboardWriteLog: Send + Sync {
    fn record_own_write(&self, sequence: u64);
}

/// Delay between pasting and restoring the clipboard (apps read it asynchronously).
const PASTE_SETTLE: Duration = Duration::from_millis(250);

/// Applies an edit to the focused application. Blocking; run off the async runtime.
pub fn apply_edit(
    platform: &dyn PlatformAdapter,
    plan: &EditPlan,
    own_writes: &dyn ClipboardWriteLog,
) -> Result<(), PlatformError> {
    match plan {
        EditPlan::Insert { text } | EditPlan::ReplaceSelection { text } => insert_text(platform, text, own_writes),
        EditPlan::ReplaceBeforeCaret { expected_tail, replacement } => {
            let current = platform.focused_input(ReadLimits::default())?.ok_or(PlatformError::NoFocusedElement)?;
            if !current.text_before_caret.ends_with(expected_tail.as_str()) {
                return Err(PlatformError::Failed("the text changed before the suggestion was applied".into()));
            }
            platform.press_key(Key::Backspace, grapheme_count(expected_tail))?;
            insert_text(platform, replacement, own_writes)
        }
        EditPlan::ReplaceAll { text } => {
            platform.select_all()?;
            insert_text(platform, text, own_writes)
        }
    }
}

/// Inserts text at the caret: typed directly when it is a single line, pasted
/// when it spans lines (typing a newline would send a chat message).
fn insert_text(
    platform: &dyn PlatformAdapter,
    text: &str,
    own_writes: &dyn ClipboardWriteLog,
) -> Result<(), PlatformError> {
    if text.is_empty() {
        return Ok(());
    }
    if !text.contains('\n') {
        return platform.type_text(text);
    }
    if platform.clipboard_has_non_text() {
        // Never destroy images or files on the clipboard: type line by line instead.
        return type_lines(platform, text);
    }
    let saved = platform.clipboard_text(usize::MAX).ok().flatten();
    let sequence = platform.set_clipboard_text(text)?;
    own_writes.record_own_write(sequence);
    let pasted = platform.paste();
    if pasted.is_ok() {
        thread::sleep(PASTE_SETTLE);
    }
    // Restore only if nobody else changed the clipboard meanwhile.
    if platform.clipboard_sequence() == sequence {
        if let Some(previous) = saved {
            let restored = platform.set_clipboard_text(&previous)?;
            own_writes.record_own_write(restored);
        }
    }
    match pasted {
        Ok(()) => Ok(()),
        // The app offers no paste command we can trigger: type instead.
        Err(PlatformError::NotSupported(_) | PlatformError::Failed(_)) => type_lines(platform, text),
        Err(error) => Err(error),
    }
}

/// Types multi-line text, using Shift+Enter for line breaks.
fn type_lines(platform: &dyn PlatformAdapter, text: &str) -> Result<(), PlatformError> {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            platform.press_key(Key::ShiftEnter, 1)?;
        }
        if !line.is_empty() {
            platform.type_text(line)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::detect;

    #[test]
    fn spelling_correction_for_spec_example() {
        let text = "I wanted to inform you that the testing is completd.";
        let c = spelling_correction(text, &detect(text), &HashSet::new()).unwrap();
        assert_eq!(c.original_tail, "completd.");
        assert_eq!(c.corrected_tail, "completed.");
        assert_eq!((c.display_from.as_str(), c.display_to.as_str()), ("completd", "completed"));
        assert_eq!(c.source, CorrectionSource::LocalSpelling);
    }

    #[test]
    fn multiple_misspellings_are_corrected_together() {
        let text = "Done. I will recieve the adress tomorow";
        let c = spelling_correction(&format!("{text} "), &detect(text), &HashSet::new()).unwrap();
        assert_eq!(c.original_tail, "recieve the adress tomorow ");
        assert_eq!(c.corrected_tail, "receive the address tomorrow ");
        assert_eq!(c.changes, 3);
    }

    #[test]
    fn word_being_typed_is_not_flagged() {
        let text = "the testing is completd";
        assert!(spelling_correction(text, &detect(text), &HashSet::new()).is_none());
        let text = "the testing is completd ";
        assert!(spelling_correction(text, &detect(text), &HashSet::new()).is_some());
    }

    #[test]
    fn transliterated_text_gets_no_spelling_suggestions() {
        for text in ["bhai kal 10 baje milte hai ", "mala ha issue samjat nahiye. "] {
            assert!(spelling_correction(text, &detect(text), &HashSet::new()).is_none(), "{text}");
        }
    }

    #[test]
    fn grammar_risk_heuristics() {
        assert!(grammar_risk("i think we should ship", None));
        assert!(grammar_risk("We dont have the logs yet.", None));
        assert!(grammar_risk("He don't know about it.", None));
        assert!(grammar_risk("Please review the the document.", None));
        assert!(grammar_risk("It was a error in the config.", None));
        assert!(grammar_risk("please send the report.", Some(IntentSubtype::Email)));
        assert!(!grammar_risk("please send the report.", Some(IntentSubtype::CasualMessage)));
        assert!(!grammar_risk("We shipped the release on time.", None));
        assert!(!grammar_risk("It was an honest mistake.", None));
        assert!(!grammar_risk("That is a one-time fix.", None));
    }

    #[test]
    fn grammar_candidates_are_complete_risky_sentences() {
        let checked = HashSet::new();
        let english = LanguageProfile::english();
        assert_eq!(
            grammar_candidate("Hi. We dont have the logs yet.", &english, None, &checked).as_deref(),
            Some("We dont have the logs yet.")
        );
        assert!(grammar_candidate("We dont have the logs yet", &english, None, &checked).is_none(), "unfinished");
        assert!(grammar_candidate("We shipped the release on time.", &english, None, &checked).is_none(), "no risk");
        let hinglish_text = "mujhe nahi pata ki logs kyun nahi mile, dont know.";
        let hinglish = detect(hinglish_text);
        assert!(hinglish.label.indic_dominant(), "{hinglish:?}");
        assert!(grammar_candidate(hinglish_text, &hinglish, None, &checked).is_none(), "indic");
        let mut seen = HashSet::new();
        seen.insert(fnv1a64("We dont have the logs yet.".as_bytes()));
        assert!(grammar_candidate("We dont have the logs yet.", &english, None, &seen).is_none(), "already checked");
    }

    #[test]
    fn ai_corrections() {
        let c = correction_from_ai("We dont have the logs yet.", "We don't have the logs yet.").unwrap();
        assert_eq!((c.display_from.as_str(), c.display_to.as_str()), ("dont", "don't"));
        assert_eq!(c.source, CorrectionSource::AiGrammar);
        assert!(correction_from_ai("Fine as is.", "Fine as is.").is_none());
        assert!(
            correction_from_ai("Short.", "A completely different and much longer rewrite of the sentence.").is_none()
        );
    }

    #[test]
    fn single_line_text_is_typed() {
        let platform = crate::testing::FakePlatform::default();
        let log = crate::testing::RecordingClipboardLog::default();
        apply_edit(&platform, &EditPlan::Insert { text: "hello world".into() }, &log).unwrap();
        assert_eq!(platform.typed(), vec!["hello world".to_string()]);
    }

    #[test]
    fn multi_line_text_is_pasted_and_clipboard_restored() {
        let platform = crate::testing::FakePlatform::default();
        platform.set_focus(Some(crate::testing::focused_input(crate::platform::AppInfo::new("a", "A"), 1, "")));
        platform.set_clipboard_text("user's copy").unwrap();
        let log = crate::testing::RecordingClipboardLog::default();
        apply_edit(&platform, &EditPlan::Insert { text: "line one\nline two".into() }, &log).unwrap();
        assert!(platform.actions().contains(&crate::testing::PlatformAction::Paste));
        assert_eq!(platform.text_before_caret().as_deref(), Some("line one\nline two"));
        assert_eq!(platform.clipboard_text(100).unwrap().as_deref(), Some("user's copy"), "clipboard restored");
        assert_eq!(log.sequences.lock().unwrap().len(), 2, "both writes are marked as Mote's own");
    }

    #[test]
    fn failed_paste_falls_back_to_typing_lines() {
        let platform = crate::testing::FakePlatform::default();
        platform.paste_fails.store(true, std::sync::atomic::Ordering::SeqCst);
        platform.set_clipboard_text("user's copy").unwrap();
        let log = crate::testing::RecordingClipboardLog::default();
        apply_edit(&platform, &EditPlan::Insert { text: "a\nb".into() }, &log).unwrap();
        assert_eq!(platform.typed(), vec!["a".to_string(), "b".to_string()]);
        assert!(platform.actions().contains(&crate::testing::PlatformAction::Key(Key::ShiftEnter, 1)));
        assert_eq!(platform.clipboard_text(100).unwrap().as_deref(), Some("user's copy"));
    }

    #[test]
    fn replace_all_selects_then_inserts() {
        let platform = crate::testing::FakePlatform::default();
        let log = crate::testing::RecordingClipboardLog::default();
        apply_edit(&platform, &EditPlan::ReplaceAll { text: "better prompt".into() }, &log).unwrap();
        assert_eq!(
            platform.actions(),
            vec![
                crate::testing::PlatformAction::SelectAll,
                crate::testing::PlatformAction::Typed("better prompt".into())
            ]
        );
    }

    #[test]
    fn writing_scope() {
        assert!(writing_applies(IntentKind::Conversation));
        assert!(!writing_applies(IntentKind::Code));
        assert!(!writing_applies(IntentKind::Command));
    }
}
