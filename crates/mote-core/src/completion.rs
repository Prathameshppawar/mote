//! Inline completion: when to ask, how to clean the answer, and how a visible
//! suggestion follows the user's typing.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::Instant;

use crate::intent::IntentKind;
use crate::language::lexicon;
use crate::platform::FocusedInput;
use crate::settings::CompletionSettings;
use crate::spelling::dictionary::EnglishDictionary;
use crate::text::{ends_mid_word, fnv1a64, last_token, normalize_whitespace, tail_chars, tokenize};

/// Characters of context sent with a completion request.
pub const COMPLETION_CONTEXT_CHARS: usize = 600;

/// Why a completion was not requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Disabled,
    IntentNotSupported,
    CaretNotAtEnd,
    HasSelection,
    TooShort,
    SentenceFinished,
    NewLine,
    TooSoon,
    Dismissed,
}

/// Decides whether the current input deserves a completion request.
pub fn should_complete(
    input: &FocusedInput,
    intent: IntentKind,
    settings: &CompletionSettings,
    since_last_request: Option<Duration>,
    dismissed: bool,
) -> Result<(), SkipReason> {
    if !settings.enabled {
        return Err(SkipReason::Disabled);
    }
    let allowed = match intent {
        IntentKind::Conversation => settings.in_conversations,
        IntentKind::Prompt => settings.in_prompts,
        IntentKind::Note => settings.in_notes,
        IntentKind::Unknown => settings.in_unknown,
        IntentKind::Code | IntentKind::Command | IntentKind::Search | IntentKind::Form => false,
    };
    if !allowed {
        return Err(SkipReason::IntentNotSupported);
    }
    if !input.caret_at_end() {
        return Err(SkipReason::CaretNotAtEnd);
    }
    if input.selected_text.as_deref().is_some_and(|s| !s.is_empty()) {
        return Err(SkipReason::HasSelection);
    }
    let text = input.text_before_caret.as_str();
    if text.ends_with('\n') {
        return Err(SkipReason::NewLine);
    }
    let trimmed = text.trim();
    if trimmed.chars().count() < settings.min_chars as usize || tokenize(trimmed).len() < 2 {
        return Err(SkipReason::TooShort);
    }
    if trimmed.ends_with(['.', '!', '?', '।']) {
        return Err(SkipReason::SentenceFinished);
    }
    if since_last_request.is_some_and(|d| d < Duration::from_millis(u64::from(settings.min_interval_ms))) {
        return Err(SkipReason::TooSoon);
    }
    if dismissed {
        return Err(SkipReason::Dismissed);
    }
    Ok(())
}

const REFUSALS: &[&str] =
    &["as an ai", "i'm sorry", "i am sorry", "i cannot", "i can't help", "sure, here", "here is", "here's the"];

/// Cleans a raw model continuation for insertion after `before`.
///
/// Returns `None` when nothing useful remains. The result may begin with a
/// space when one is needed between `before` and the continuation.
pub fn clean_completion(raw: &str, before: &str, max_words: usize, truncated: bool) -> Option<String> {
    let mut text = raw.replace('\r', "");
    // Keep the first paragraph only.
    if let Some(idx) = text.find("\n\n") {
        text.truncate(idx);
    }
    let mut text = text.trim_end().to_string();
    // Strip wrapping quotes and leading ellipses.
    let trimmed = text.trim_start();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('“') && trimmed.ends_with('”')))
    {
        let first_len = trimmed.chars().next().map_or(1, char::len_utf8);
        let last_len = trimmed.chars().last().map_or(1, char::len_utf8);
        text = trimmed[first_len..trimmed.len() - last_len].to_string();
    }
    let text = text.trim_start_matches(['…']).trim_start_matches("...").to_string();

    // Remove an echo of the end of `before` (models sometimes repeat the tail).
    let text = strip_echo(&text, before);
    let lower = text.trim().to_lowercase();
    if lower.is_empty() || REFUSALS.iter().any(|r| lower.starts_with(r)) {
        return None;
    }
    if !text.chars().any(char::is_alphanumeric) {
        return None;
    }

    // Leading whitespace: exactly what the join needs.
    let body = text.trim_start();
    let joined = if before.is_empty()
        || before.ends_with(char::is_whitespace)
        || body.starts_with(|c: char| ".,;:!?)]}'’…".contains(c))
    {
        body.to_string()
    } else if ends_mid_word(before) && body.starts_with(char::is_alphanumeric) {
        if continues_word(before, body) {
            body.to_string()
        } else {
            format!(" {body}")
        }
    } else if before.ends_with(['(', '[', '{', '"', '“', '\'', '/', '-']) {
        body.to_string()
    } else {
        format!(" {body}")
    };

    let mut result = limit_words(&joined, max_words);
    if truncated {
        // The model was cut off: drop a trailing partial word.
        if let Some(cut) = result.rfind(char::is_whitespace) {
            if result.ends_with(char::is_alphanumeric) {
                result.truncate(cut);
            }
        }
    }
    let result = result.trim_end().to_string();
    if result.trim().is_empty() {
        return None;
    }
    // A suggestion identical to what is already written is useless.
    if before.trim_end().ends_with(result.trim()) && result.trim().chars().count() > 3 {
        return None;
    }
    Some(result)
}

/// Whether `body` completes the partial word at the end of `before`.
fn continues_word(before: &str, body: &str) -> bool {
    let Some(last) = last_token(before) else { return false };
    let last_lower = last.text.to_lowercase();
    let first_word: String = body.chars().take_while(|c| c.is_alphanumeric()).collect();
    let combined = format!("{last_lower}{}", first_word.to_lowercase());
    let dictionary = EnglishDictionary::global();
    let last_is_word = dictionary.contains(&last_lower) || lexicon::is_indic_word(&last_lower);
    let combined_is_word = dictionary.contains(&combined);
    let first_is_word =
        dictionary.contains(&first_word.to_lowercase()) || lexicon::is_indic_word(&first_word.to_lowercase());
    if combined_is_word && !last_is_word {
        return true;
    }
    if combined_is_word && !first_is_word {
        return true;
    }
    // Unknown fragment followed by an unknown suffix: probably one word.
    !last_is_word && !first_is_word && !combined_is_word && first_word.len() <= 4
}

fn strip_echo(text: &str, before: &str) -> String {
    let body = text.trim_start();
    let tail = tail_chars(before, 80);
    let tail_chars_vec: Vec<(usize, char)> = tail.char_indices().collect();
    for (start, _) in tail_chars_vec {
        let suffix = &tail[start..];
        if suffix.trim().chars().count() >= 4 && body.starts_with(suffix.trim_start()) {
            return body[suffix.trim_start().len()..].to_string();
        }
    }
    text.to_string()
}

fn limit_words(text: &str, max_words: usize) -> String {
    let tokens = tokenize(text);
    if tokens.len() <= max_words || max_words == 0 {
        return text.to_string();
    }
    let end = tokens[max_words - 1].end;
    // Keep punctuation that directly follows the last word.
    let rest = &text[end..];
    let punct: usize = rest.chars().take_while(|c| ".,;:!?".contains(*c)).map(char::len_utf8).sum();
    text[..end + punct].to_string()
}

/// How a visible suggestion relates to new text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionUpdate {
    /// Still valid; the remaining text to show.
    Keep(String),
    /// The user typed the whole suggestion.
    Consumed,
    /// The text diverged; hide the suggestion.
    Invalid,
}

/// Candidates for one anchor text, with type-through tracking.
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestionSet {
    pub anchor: String,
    /// Anchor key (see [`anchor_key`]) used for caching and dismissal.
    pub key: u64,
    candidates: Vec<String>,
    index: usize,
    typed: String,
}

impl SuggestionSet {
    pub fn new(anchor: String, key: u64, first: String) -> Self {
        Self { anchor, key, candidates: vec![first], index: 0, typed: String::new() }
    }

    /// Remaining text of the current candidate.
    pub fn current(&self) -> &str {
        self.candidates[self.index].strip_prefix(self.typed.as_str()).unwrap_or("")
    }

    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// The text before the caret the suggestion currently continues.
    pub fn expected_text(&self) -> String {
        format!("{}{}", self.anchor, self.typed)
    }

    /// Updates for new text before the caret.
    pub fn on_text(&mut self, text_before: &str) -> SuggestionUpdate {
        let Some(typed) = text_before.strip_prefix(self.anchor.as_str()) else {
            return SuggestionUpdate::Invalid;
        };
        let current = &self.candidates[self.index];
        if !current.starts_with(typed) {
            // Maybe another candidate matches what the user typed.
            match self.candidates.iter().position(|c| c.starts_with(typed)) {
                Some(i) => self.index = i,
                None => return SuggestionUpdate::Invalid,
            }
        }
        self.typed = typed.to_string();
        let remaining = self.current();
        if remaining.trim().is_empty() {
            SuggestionUpdate::Consumed
        } else {
            SuggestionUpdate::Keep(remaining.to_string())
        }
    }

    /// Adds a candidate (ignoring duplicates); returns whether it was new.
    pub fn push(&mut self, candidate: String) -> bool {
        let norm = normalize_whitespace(&candidate);
        if self.candidates.iter().any(|c| normalize_whitespace(c) == norm)
            || !candidate.starts_with(self.typed.as_str())
        {
            return false;
        }
        self.candidates.push(candidate);
        true
    }

    /// Moves to the next candidate consistent with typed text.
    pub fn select_next(&mut self) -> bool {
        self.step(1)
    }

    /// Moves to the previous candidate consistent with typed text.
    pub fn select_previous(&mut self) -> bool {
        self.step(-1)
    }

    fn step(&mut self, direction: isize) -> bool {
        let n = self.candidates.len() as isize;
        let mut i = self.index as isize;
        loop {
            i += direction;
            if i < 0 || i >= n {
                return false;
            }
            if self.candidates[i as usize].starts_with(self.typed.as_str()) {
                self.index = i as usize;
                return true;
            }
        }
    }
}

/// Key for caching and dismissal: app, intent and the normalized recent text.
pub fn anchor_key(app_id: &str, intent: IntentKind, text_before: &str) -> u64 {
    let tail = normalize_whitespace(tail_chars(text_before, 200));
    fnv1a64(format!("{app_id}\u{0}{}\u{0}{tail}", intent.as_str()).as_bytes())
}

/// A small LRU cache of completions, so retyping the same text costs nothing.
#[derive(Debug)]
pub struct CompletionCache {
    capacity: usize,
    ttl: Duration,
    entries: VecDeque<(u64, Instant, Vec<String>)>,
}

impl CompletionCache {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self { capacity, ttl, entries: VecDeque::new() }
    }

    pub fn get(&mut self, key: u64) -> Option<Vec<String>> {
        let now = Instant::now();
        self.entries.retain(|(_, at, _)| now.duration_since(*at) < self.ttl);
        let pos = self.entries.iter().position(|(k, _, _)| *k == key)?;
        let entry = self.entries.remove(pos)?;
        let value = entry.2.clone();
        self.entries.push_back(entry);
        Some(value)
    }

    pub fn put(&mut self, key: u64, candidates: Vec<String>) {
        self.entries.retain(|(k, _, _)| *k != key);
        self.entries.push_back((key, Instant::now(), candidates));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Anchors the user dismissed recently; Mote will not re-suggest for them.
#[derive(Debug, Default)]
pub struct DismissedAnchors {
    keys: VecDeque<u64>,
}

impl DismissedAnchors {
    const CAPACITY: usize = 128;

    pub fn insert(&mut self, key: u64) {
        if !self.keys.contains(&key) {
            self.keys.push_back(key);
            if self.keys.len() > Self::CAPACITY {
                self.keys.pop_front();
            }
        }
    }

    pub fn contains(&self, key: u64) -> bool {
        self.keys.contains(&key)
    }

    pub fn clear(&mut self) {
        self.keys.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{AppInfo, InputRole};

    fn input(before: &str, after: &str) -> FocusedInput {
        FocusedInput {
            app: AppInfo::new("com.tinyspeck.slackmacgap", "Slack"),
            role: InputRole::TextArea,
            is_secure: false,
            is_multiline: true,
            is_web_content: true,
            placeholder: None,
            label: None,
            text_before_caret: before.into(),
            text_after_caret: after.into(),
            selected_text: None,
            total_length: None,
            caret_rect: None,
            element_key: 1,
        }
    }

    #[test]
    fn completion_policy() {
        let s = CompletionSettings::default();
        let ok = input("The deployment failed because", "");
        assert_eq!(should_complete(&ok, IntentKind::Conversation, &s, None, false), Ok(()));
        assert_eq!(should_complete(&ok, IntentKind::Code, &s, None, false), Err(SkipReason::IntentNotSupported));
        assert_eq!(should_complete(&ok, IntentKind::Search, &s, None, false), Err(SkipReason::IntentNotSupported));
        assert_eq!(
            should_complete(&input("hi", ""), IntentKind::Conversation, &s, None, false),
            Err(SkipReason::TooShort)
        );
        assert_eq!(
            should_complete(
                &input("The deployment failed because", " of x"),
                IntentKind::Conversation,
                &s,
                None,
                false
            ),
            Err(SkipReason::CaretNotAtEnd)
        );
        assert_eq!(
            should_complete(&input("The deployment failed today.", ""), IntentKind::Conversation, &s, None, false),
            Err(SkipReason::SentenceFinished)
        );
        assert_eq!(
            should_complete(&ok, IntentKind::Conversation, &s, Some(Duration::from_millis(100)), false),
            Err(SkipReason::TooSoon)
        );
        assert_eq!(should_complete(&ok, IntentKind::Conversation, &s, None, true), Err(SkipReason::Dismissed));
        let mut sel = ok.clone();
        sel.selected_text = Some("failed".into());
        assert_eq!(should_complete(&sel, IntentKind::Conversation, &s, None, false), Err(SkipReason::HasSelection));
        let disabled = CompletionSettings { enabled: false, ..CompletionSettings::default() };
        assert_eq!(should_complete(&ok, IntentKind::Conversation, &disabled, None, false), Err(SkipReason::Disabled));
        assert_eq!(
            should_complete(&input("Line one is here\n", ""), IntentKind::Note, &s, None, false),
            Err(SkipReason::NewLine)
        );
    }

    #[test]
    fn spec_example_continuation() {
        let c = clean_completion(
            "the Redis container was unavailable during startup.",
            "The deployment failed because",
            12,
            false,
        );
        assert_eq!(c.as_deref(), Some(" the Redis container was unavailable during startup."));
    }

    #[test]
    fn joins_partial_words_without_a_space() {
        assert_eq!(clean_completion("ent failed again", "The deploym", 12, false).as_deref(), Some("ent failed again"));
        assert_eq!(clean_completion("the build", "because", 12, false).as_deref(), Some(" the build"));
        assert_eq!(clean_completion("hai", "bhai kal milte", 12, false).as_deref(), Some(" hai"));
    }

    #[test]
    fn respects_trailing_whitespace_and_punctuation() {
        assert_eq!(clean_completion(" the build", "because ", 12, false).as_deref(), Some("the build"));
        assert_eq!(clean_completion(", and then", "It failed", 12, false).as_deref(), Some(", and then"));
        assert_eq!(clean_completion("we retried", "Afterwards,", 12, false).as_deref(), Some(" we retried"));
    }

    #[test]
    fn strips_echo_quotes_and_refusals() {
        assert_eq!(
            clean_completion("failed because the cache was cold", "The deployment failed because", 12, false)
                .as_deref(),
            Some(" the cache was cold")
        );
        assert_eq!(
            clean_completion("\"the cache was cold\"", "because", 12, false).as_deref(),
            Some(" the cache was cold")
        );
        assert_eq!(clean_completion("As an AI, I cannot", "because", 12, false), None);
        assert_eq!(clean_completion("   ", "because", 12, false), None);
        assert_eq!(clean_completion("...", "because", 12, false), None);
    }

    #[test]
    fn limits_length_and_trims_truncation() {
        let c = clean_completion("one two three four five six", "Count:", 3, false).unwrap();
        assert_eq!(c, " one two three");
        let c = clean_completion("the server was unreach", "It failed because", 12, true).unwrap();
        assert_eq!(c, " the server was");
        assert_eq!(clean_completion("first para\n\nsecond para", "Note", 12, false).as_deref(), Some(" first para"));
    }

    #[test]
    fn suggestion_follows_typing() {
        let mut set = SuggestionSet::new("The build".into(), 1, " failed on CI".into());
        assert_eq!(set.on_text("The build"), SuggestionUpdate::Keep(" failed on CI".into()));
        assert_eq!(set.on_text("The build fa"), SuggestionUpdate::Keep("iled on CI".into()));
        assert_eq!(set.expected_text(), "The build fa");
        assert_eq!(set.on_text("The build failed on CI"), SuggestionUpdate::Consumed);
        let mut set = SuggestionSet::new("The build".into(), 1, " failed on CI".into());
        assert_eq!(set.on_text("The build passed"), SuggestionUpdate::Invalid);
        assert_eq!(set.on_text("The bui"), SuggestionUpdate::Invalid);
    }

    #[test]
    fn cycling_candidates() {
        let mut set = SuggestionSet::new("We".into(), 1, " shipped it".into());
        assert!(set.push(" fixed the bug".into()));
        assert!(!set.push(" shipped  it".into()), "duplicates ignored");
        assert!(set.select_next());
        assert_eq!(set.current(), " fixed the bug");
        assert!(!set.select_next(), "no wrap past the end");
        assert!(set.select_previous());
        assert_eq!(set.current(), " shipped it");
        // Typing selects the candidate that matches.
        assert_eq!(set.on_text("We fix"), SuggestionUpdate::Keep("ed the bug".into()));
        assert_eq!(set.index(), 1);
    }

    #[test]
    fn cache_is_lru_with_ttl() {
        let mut cache = CompletionCache::new(2, Duration::from_secs(60));
        cache.put(1, vec!["a".into()]);
        cache.put(2, vec!["b".into()]);
        assert_eq!(cache.get(1), Some(vec!["a".to_string()]));
        cache.put(3, vec!["c".into()]);
        assert_eq!(cache.get(2), None, "least recently used evicted");
        assert!(cache.get(1).is_some() && cache.get(3).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn cache_entries_expire() {
        let mut cache = CompletionCache::new(4, Duration::from_secs(10));
        cache.put(1, vec!["a".into()]);
        tokio::time::advance(Duration::from_secs(11)).await;
        assert_eq!(cache.get(1), None);
    }

    #[test]
    fn anchor_keys_normalize_whitespace() {
        let a = anchor_key("app", IntentKind::Conversation, "hello  world");
        let b = anchor_key("app", IntentKind::Conversation, "hello world");
        let c = anchor_key("other", IntentKind::Conversation, "hello world");
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut d = DismissedAnchors::default();
        d.insert(a);
        assert!(d.contains(b));
    }
}
