//! Intent classification: what kind of writing is the user doing?
//!
//! Mote assists a chat message differently from an AI prompt, a terminal command
//! or a search box. Classification is deterministic first: the application, the
//! focused field, its placeholder, the text itself, its language and recent
//! context each add weighted evidence. Only when that evidence is inconclusive
//! (and the user allows it) does Mote ask the classification model, and the
//! result is cached per input field.

pub mod apps;

use serde::{Deserialize, Serialize};

use crate::context::clipboard::{is_code_line, ClipboardKind};
use crate::language::LanguageProfile;
use crate::platform::InputRole;
use crate::text::{tokenize, word_count};
use apps::AppCategory;

/// The kind of writing context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    Conversation,
    Prompt,
    Code,
    Command,
    Note,
    Search,
    Form,
    Unknown,
}

impl IntentKind {
    pub const ALL: [IntentKind; 8] = [
        Self::Conversation,
        Self::Prompt,
        Self::Code,
        Self::Command,
        Self::Note,
        Self::Search,
        Self::Form,
        Self::Unknown,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Prompt => "prompt",
            Self::Code => "code",
            Self::Command => "command",
            Self::Note => "note",
            Self::Search => "search",
            Self::Form => "form",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s.trim().to_lowercase())
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(7)
    }
}

/// Finer-grained type of a conversation or prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum IntentSubtype {
    // Conversation
    Email,
    Chat,
    ProfessionalMessage,
    CasualMessage,
    // Prompt
    Coding,
    Research,
    Reasoning,
    General,
}

impl IntentSubtype {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Chat => "chat",
            Self::ProfessionalMessage => "professional_message",
            Self::CasualMessage => "casual_message",
            Self::Coding => "coding",
            Self::Research => "research",
            Self::Reasoning => "reasoning",
            Self::General => "general",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Email,
            Self::Chat,
            Self::ProfessionalMessage,
            Self::CasualMessage,
            Self::Coding,
            Self::Research,
            Self::Reasoning,
            Self::General,
        ]
        .into_iter()
        .find(|k| k.as_str() == s.trim().to_lowercase())
    }

    /// Whether this subtype belongs to `kind`.
    pub fn belongs_to(self, kind: IntentKind) -> bool {
        match kind {
            IntentKind::Conversation => {
                matches!(self, Self::Email | Self::Chat | Self::ProfessionalMessage | Self::CasualMessage)
            }
            IntentKind::Prompt => matches!(self, Self::Coding | Self::Research | Self::Reasoning | Self::General),
            _ => false,
        }
    }
}

/// Where an assessment came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum IntentSource {
    Deterministic,
    Ai,
    UserRule,
}

/// The classification result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct IntentAssessment {
    pub kind: IntentKind,
    pub subtype: Option<IntentSubtype>,
    pub confidence: f32,
    pub source: IntentSource,
    /// Names of the signals that contributed (no user content), for diagnostics.
    pub signals: Vec<String>,
}

impl IntentAssessment {
    pub fn unknown() -> Self {
        Self {
            kind: IntentKind::Unknown,
            subtype: None,
            confidence: 0.0,
            source: IntentSource::Deterministic,
            signals: Vec::new(),
        }
    }
}

/// Everything the classifier looks at. Text is bounded by the caller.
#[derive(Debug, Clone)]
pub struct IntentSignals<'a> {
    pub category: AppCategory,
    pub role: Option<InputRole>,
    pub is_multiline: bool,
    pub placeholder: Option<&'a str>,
    pub label: Option<&'a str>,
    pub text: &'a str,
    pub language: &'a LanguageProfile,
    pub clipboard_kind: Option<ClipboardKind>,
    pub previous_category: Option<AppCategory>,
    /// The user told Mote how to treat this application.
    pub user_override: Option<IntentKind>,
}

#[derive(Default)]
struct Scores {
    values: [f32; 8],
    signals: Vec<String>,
}

impl Scores {
    fn add(&mut self, kind: IntentKind, weight: f32, signal: &str) {
        self.values[kind.index()] += weight;
        self.signals.push(signal.to_string());
    }

    fn ranked(&self) -> Vec<(IntentKind, f32)> {
        let mut ranked: Vec<(IntentKind, f32)> = IntentKind::ALL.iter().map(|k| (*k, self.values[k.index()])).collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        ranked
    }
}

const PROMPT_HINTS: &[&str] = &[
    "ask anything",
    "ask chatgpt",
    "message chatgpt",
    "ask claude",
    "reply to claude",
    "how can i help",
    "ask copilot",
    "ask gemini",
    "enter a prompt",
    "prompt",
    "ask a question",
    "plan, search, build",
    "ask perplexity",
    "ask ai",
    "describe what you want",
    "ask me anything",
    "type your question",
    "ask follow-up",
    "ask a follow-up",
    "what do you want to build",
    "message copilot",
    "message claude",
    "message gemini",
    "ask grok",
    "ask meta ai",
    "queue another message",
    "tell claude",
    "ask deepseek",
    "edit code",
    "add a follow-up",
    "ask a side question",
];
/// Accessible labels of AI chat boxes inside IDEs: Claude Code's "Message
/// input", VS Code's "Chat input" (Copilot) and "Inline Chat Input". Only
/// trusted in IDEs, where a message box is an assistant's, never a person's.
const IDE_CHAT_HINTS: &[&str] = &["message input", "chat input"];
const CONVERSATION_HINTS: &[&str] = &[
    "message #",
    "message @",
    "type a message",
    "write a message",
    "send a message",
    "reply",
    "write a reply",
    "new message",
    "comment",
    "add a comment",
    "write a comment",
    "compose",
    "jot something",
    "message ",
    "write something",
    "chat",
];
const SEARCH_HINTS: &[&str] =
    &["search", "find", "filter", "go to", "address and search", "search or type", "search or enter", "look up"];
const FORM_HINTS: &[&str] = &[
    "name",
    "email",
    "phone",
    "address",
    "city",
    "zip",
    "postal",
    "company",
    "card",
    "username",
    "password",
    "date",
    "subject",
    "recipient",
    "amount",
    "url",
    "website",
];
const PROMPT_VERBS: &[&str] = &[
    "write",
    "create",
    "generate",
    "explain",
    "fix",
    "refactor",
    "summarize",
    "summarise",
    "translate",
    "help",
    "give",
    "list",
    "compare",
    "analyze",
    "analyse",
    "act",
    "draft",
    "make",
    "convert",
    "debug",
    "implement",
    "build",
    "design",
    "review",
    "optimize",
    "optimise",
    "find",
    "tell",
    "describe",
    "suggest",
    "rewrite",
    "improve",
    "plan",
    "outline",
    "how",
    "what",
    "why",
    "can",
    "could",
    "please",
    "show",
    "teach",
    "brainstorm",
    "evaluate",
    "solve",
    "calculate",
    "prove",
    "research",
    "investigate",
    "update",
    "add",
    "remove",
    "port",
    "migrate",
    "test",
];
const PROMPT_PHRASES: &[&str] = &[
    "you are a",
    "act as",
    "step by step",
    "in detail",
    "for me",
    "pros and cons",
    "best way to",
    "how do i",
    "how to",
    "explain",
    "give me",
    "write a",
    "write an",
    "create a",
    "generate a",
    "help me",
    "i want you to",
    "based on",
    "the following",
];
const SHELL_COMMANDS: &[&str] = &[
    "git",
    "npm",
    "npx",
    "pnpm",
    "yarn",
    "cargo",
    "docker",
    "kubectl",
    "cd",
    "ls",
    "mkdir",
    "rm",
    "cp",
    "mv",
    "cat",
    "grep",
    "sudo",
    "brew",
    "apt",
    "pip",
    "pip3",
    "python",
    "python3",
    "node",
    "curl",
    "wget",
    "ssh",
    "scp",
    "make",
    "go",
    "rustup",
    "chmod",
    "chown",
    "echo",
    "export",
    "source",
    "tail",
    "head",
    "find",
    "ps",
    "kill",
    "terraform",
    "aws",
    "gcloud",
    "az",
    "helm",
    "systemctl",
    "journalctl",
    "dir",
    "cls",
    "ipconfig",
    "ping",
    "deno",
    "bun",
    "uv",
];
const GREETINGS: &[&str] =
    &["hi ", "hi,", "hello", "dear ", "hey ", "good morning", "good afternoon", "good evening", "namaste"];
const SIGNOFFS: &[&str] = &["regards", "thanks,", "thank you", "best,", "sincerely", "cheers", "warm wishes"];
const CASUAL_MARKERS: &[&str] = &[
    "lol", "haha", "hahaha", "bro", "bhai", "yaar", "dude", "gonna", "wanna", "ok", "okay", "cool", "pls", "thx",
    "btw", "omg", "yep", "nope", "ya", "yeah", "sup",
];
const FORMAL_MARKERS: &[&str] = &[
    "please",
    "kindly",
    "regards",
    "would",
    "could",
    "meeting",
    "client",
    "deadline",
    "update",
    "attached",
    "following",
    "discussed",
    "team",
    "project",
    "review",
    "schedule",
    "proposal",
    "request",
    "appreciate",
    "confirm",
    "available",
    "regarding",
];
const CODING_WORDS: &[&str] = &[
    "code",
    "function",
    "bug",
    "error",
    "exception",
    "api",
    "script",
    "python",
    "javascript",
    "typescript",
    "rust",
    "java",
    "sql",
    "regex",
    "compile",
    "build",
    "deploy",
    "test",
    "tests",
    "refactor",
    "debug",
    "stack",
    "repo",
    "git",
    "docker",
    "class",
    "method",
    "variable",
    "endpoint",
    "query",
    "react",
    "component",
    "database",
    "server",
    "crash",
    "null",
    "undefined",
    "fix",
];
const RESEARCH_WORDS: &[&str] = &[
    "research",
    "sources",
    "cite",
    "citations",
    "compare",
    "comparison",
    "pros",
    "cons",
    "market",
    "history",
    "latest",
    "studies",
    "study",
    "paper",
    "papers",
    "survey",
    "overview",
    "trends",
    "alternatives",
    "best",
];
const REASONING_WORDS: &[&str] = &[
    "why",
    "prove",
    "calculate",
    "solve",
    "reason",
    "logic",
    "math",
    "equation",
    "probability",
    "decide",
    "should",
    "tradeoff",
    "trade-off",
    "derive",
    "estimate",
    "puzzle",
    "think",
    "plan",
];

fn hint_matches(hint: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hint.contains(n))
}

fn count_words_in(tokens: &[String], vocabulary: &[&str]) -> usize {
    tokens.iter().filter(|t| vocabulary.contains(&t.as_str())).count()
}

/// Share of non-empty lines that look like code.
fn code_ratio(text: &str) -> f32 {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return 0.0;
    }
    lines.iter().filter(|l| is_code_line(l)).count() as f32 / lines.len() as f32
}

fn looks_like_command(text: &str) -> (bool, bool) {
    let first_line = text.lines().last().unwrap_or("").trim();
    let stripped = first_line.trim_start_matches(['$', '>', '#', '%']).trim_start();
    let stripped = stripped.strip_prefix("PS ").unwrap_or(stripped);
    let first = stripped.split_whitespace().next().unwrap_or("").to_lowercase();
    let is_cmd = SHELL_COMMANDS.contains(&first.as_str());
    let has_flags = stripped.contains(" -") || stripped.contains(" | ") || stripped.contains(" && ");
    (is_cmd, has_flags)
}

/// Deterministically classifies the writing context.
pub fn classify(signals: &IntentSignals<'_>) -> IntentAssessment {
    if let Some(kind) = signals.user_override {
        return IntentAssessment {
            kind,
            subtype: subtype_for(kind, signals),
            confidence: 1.0,
            source: IntentSource::UserRule,
            signals: vec!["user_rule".into()],
        };
    }
    let mut s = Scores::default();

    // 1. Application category: the strongest single signal.
    match signals.category {
        AppCategory::Email => s.add(IntentKind::Conversation, 3.0, "app:email"),
        AppCategory::Chat => s.add(IntentKind::Conversation, 3.0, "app:chat"),
        AppCategory::AiAssistant => s.add(IntentKind::Prompt, 3.5, "app:ai_assistant"),
        AppCategory::Ide => s.add(IntentKind::Code, 2.5, "app:ide"),
        AppCategory::Terminal => s.add(IntentKind::Command, 4.0, "app:terminal"),
        AppCategory::Notes => s.add(IntentKind::Note, 2.5, "app:notes"),
        AppCategory::Launcher => s.add(IntentKind::Search, 4.0, "app:launcher"),
        AppCategory::Browser | AppCategory::Other => s.add(IntentKind::Unknown, 0.6, "app:generic"),
        AppCategory::PasswordManager | AppCategory::Mote => s.add(IntentKind::Form, 5.0, "app:protected"),
    }

    // 2. Field role.
    match signals.role {
        Some(InputRole::SearchField) => s.add(IntentKind::Search, 3.0, "role:search_field"),
        Some(InputRole::ComboBox) => s.add(IntentKind::Search, 1.5, "role:combo_box"),
        Some(InputRole::Terminal) => s.add(IntentKind::Command, 3.0, "role:terminal"),
        Some(InputRole::TextField) if !signals.is_multiline => {
            s.add(IntentKind::Form, 0.8, "role:single_line");
            s.add(IntentKind::Search, 0.4, "role:single_line");
        }
        Some(InputRole::TextArea | InputRole::Document) => {
            s.add(IntentKind::Conversation, 0.3, "role:multiline");
            s.add(IntentKind::Note, 0.3, "role:multiline");
            s.add(IntentKind::Prompt, 0.3, "role:multiline");
        }
        _ => {}
    }

    // 3. Placeholder and accessible label.
    let hint = format!("{} {}", signals.placeholder.unwrap_or(""), signals.label.unwrap_or("")).to_lowercase();
    let hint = hint.trim();
    if !hint.is_empty() {
        if hint_matches(hint, PROMPT_HINTS) {
            s.add(IntentKind::Prompt, 4.0, "hint:prompt");
        } else if signals.category == AppCategory::Ide && hint_matches(hint, IDE_CHAT_HINTS) {
            s.add(IntentKind::Prompt, 4.0, "hint:ide_chat");
        } else if hint_matches(hint, CONVERSATION_HINTS) {
            s.add(IntentKind::Conversation, 3.0, "hint:conversation");
        }
        if hint.contains("commit message") {
            s.add(IntentKind::Note, 3.0, "hint:commit_message");
        }
        if hint_matches(hint, SEARCH_HINTS) {
            s.add(IntentKind::Search, 3.5, "hint:search");
        }
        if !signals.is_multiline
            && FORM_HINTS.iter().any(|f| hint.split(|c: char| !c.is_alphanumeric()).any(|w| w == *f))
        {
            s.add(IntentKind::Form, 2.5, "hint:form_field");
        }
    }

    // 4. The text itself.
    let text = signals.text.trim();
    let lower = text.to_lowercase();
    let tokens: Vec<String> = tokenize(&lower).iter().map(|t| t.text.to_string()).collect();
    if !text.is_empty() {
        let first_word = tokens.first().map(String::as_str).unwrap_or("");
        if PROMPT_VERBS.contains(&first_word)
            && signals.category != AppCategory::Chat
            && signals.category != AppCategory::Email
        {
            s.add(IntentKind::Prompt, 1.5, "text:imperative");
        }
        let phrase_hits = PROMPT_PHRASES.iter().filter(|p| lower.contains(*p)).count().min(2);
        if phrase_hits > 0 {
            s.add(IntentKind::Prompt, phrase_hits as f32 * 0.8, "text:prompt_phrase");
        }
        let starts_with_greeting = GREETINGS.iter().any(|g| lower.starts_with(g));
        let has_signoff = SIGNOFFS.iter().any(|g| lower.contains(g));
        if starts_with_greeting {
            s.add(IntentKind::Conversation, 1.5, "text:greeting");
        }
        if has_signoff {
            s.add(IntentKind::Conversation, 1.0, "text:signoff");
        }
        if count_words_in(&tokens, CASUAL_MARKERS) > 0 || text.chars().any(is_emoji) {
            s.add(IntentKind::Conversation, 1.0, "text:casual");
        }
        let code = code_ratio(text);
        if code >= 0.5 {
            s.add(IntentKind::Code, 3.0, "text:code");
        } else if code >= 0.25 {
            s.add(IntentKind::Code, 1.2, "text:some_code");
        }
        let (is_cmd, has_flags) = looks_like_command(text);
        if is_cmd && !signals.is_multiline {
            s.add(IntentKind::Command, if has_flags { 3.0 } else { 2.0 }, "text:shell_command");
        }
        let words = word_count(text);
        if words <= 6 && !text.contains('\n') && !text.ends_with(['.', '!', '?']) && !starts_with_greeting {
            s.add(IntentKind::Search, 0.6, "text:short_query");
        }
        if signals.language.label.has_indic() {
            s.add(IntentKind::Conversation, 0.6, "language:indic");
        }
    }

    // 5. Surrounding context.
    if matches!(signals.clipboard_kind, Some(ClipboardKind::Code | ClipboardKind::StackTrace))
        && matches!(signals.category, AppCategory::AiAssistant | AppCategory::Ide | AppCategory::Browser)
    {
        s.add(IntentKind::Prompt, 0.5, "context:clipboard_code");
    }
    if signals.previous_category == Some(AppCategory::Email)
        && matches!(signals.category, AppCategory::AiAssistant | AppCategory::Ide)
    {
        s.add(IntentKind::Prompt, 0.3, "context:from_email");
    }

    let ranked = s.ranked();
    let (top_kind, top) = ranked[0];
    let second = ranked[1].1;
    let kind = if top <= 0.0 { IntentKind::Unknown } else { top_kind };
    let margin = top - second;
    let mut confidence = 1.0 - (-margin / 2.0).exp();
    if top < 1.5 {
        confidence *= top / 1.5;
    }
    IntentAssessment {
        kind,
        subtype: subtype_for(kind, signals),
        confidence: confidence.clamp(0.0, 1.0),
        source: IntentSource::Deterministic,
        signals: s.signals,
    }
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF)
}

/// Picks the subtype for conversations and prompts.
pub fn subtype_for(kind: IntentKind, signals: &IntentSignals<'_>) -> Option<IntentSubtype> {
    let lower = signals.text.to_lowercase();
    let tokens: Vec<String> = tokenize(&lower).iter().map(|t| t.text.to_string()).collect();
    match kind {
        IntentKind::Conversation => {
            let greeting = GREETINGS.iter().any(|g| lower.trim_start().starts_with(g));
            let signoff = SIGNOFFS.iter().any(|g| lower.contains(g));
            if signals.category == AppCategory::Email || (greeting && signoff && lower.contains('\n')) {
                return Some(IntentSubtype::Email);
            }
            let formal = count_words_in(&tokens, FORMAL_MARKERS);
            let casual = count_words_in(&tokens, CASUAL_MARKERS)
                + usize::from(signals.language.label.indic_dominant())
                + usize::from(lower.chars().any(is_emoji));
            if formal > casual && formal >= 1 {
                Some(IntentSubtype::ProfessionalMessage)
            } else if casual > formal {
                Some(IntentSubtype::CasualMessage)
            } else {
                Some(IntentSubtype::Chat)
            }
        }
        IntentKind::Prompt => {
            let coding = count_words_in(&tokens, CODING_WORDS)
                + if signals.category == AppCategory::Ide { 2 } else { 0 }
                + if matches!(signals.clipboard_kind, Some(ClipboardKind::Code | ClipboardKind::StackTrace)) {
                    1
                } else {
                    0
                }
                + if code_ratio(signals.text) > 0.2 { 2 } else { 0 };
            let research = count_words_in(&tokens, RESEARCH_WORDS);
            let reasoning = count_words_in(&tokens, REASONING_WORDS);
            let best = coding.max(research).max(reasoning);
            if best == 0 {
                Some(IntentSubtype::General)
            } else if coding == best {
                Some(IntentSubtype::Coding)
            } else if research == best {
                Some(IntentSubtype::Research)
            } else {
                Some(IntentSubtype::Reasoning)
            }
        }
        _ => None,
    }
}

/// Below this confidence an AI classification may help.
pub const AI_CLASSIFICATION_THRESHOLD: f32 = 0.45;
/// Minimum text length worth sending for classification.
pub const AI_CLASSIFICATION_MIN_CHARS: usize = 24;

/// Whether an AI classification is worth its tokens for this assessment.
pub fn needs_ai_classification(assessment: &IntentAssessment, text: &str) -> bool {
    assessment.source == IntentSource::Deterministic
        && assessment.confidence < AI_CLASSIFICATION_THRESHOLD
        && text.trim().chars().count() >= AI_CLASSIFICATION_MIN_CHARS
        && !matches!(assessment.kind, IntentKind::Command | IntentKind::Form)
}

/// Parses the classification model's JSON answer.
///
/// Tolerates prose around the JSON object, unknown labels and subtypes that do
/// not belong to the kind.
pub fn parse_ai_classification(raw: &str) -> Option<IntentAssessment> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    let value: serde_json::Value = serde_json::from_str(raw.get(start..=end)?).ok()?;
    let kind = value.get("kind").and_then(|v| v.as_str()).and_then(IntentKind::parse).unwrap_or(IntentKind::Unknown);
    let subtype =
        value.get("subtype").and_then(|v| v.as_str()).and_then(IntentSubtype::parse).filter(|s| s.belongs_to(kind));
    let confidence = value.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.6).clamp(0.0, 1.0) as f32;
    Some(IntentAssessment { kind, subtype, confidence, source: IntentSource::Ai, signals: vec!["ai".into()] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::detect;

    struct Case<'a> {
        category: AppCategory,
        role: Option<InputRole>,
        multiline: bool,
        placeholder: Option<&'a str>,
        text: &'a str,
    }

    fn run(c: Case<'_>) -> IntentAssessment {
        let language = detect(c.text);
        classify(&IntentSignals {
            category: c.category,
            role: c.role,
            is_multiline: c.multiline,
            placeholder: c.placeholder,
            label: None,
            text: c.text,
            language: &language,
            clipboard_kind: None,
            previous_category: None,
            user_override: None,
        })
    }

    #[test]
    fn ai_chat_inputs_inside_an_ide_are_prompts() {
        let language = detect("refactor the parser so errors carry line numbers");
        for placeholder in ["Ask Claude to edit…", "Queue another message…", "Ask Copilot or type / for commands"] {
            let a = classify(&IntentSignals {
                category: AppCategory::Ide,
                role: Some(InputRole::TextArea),
                is_multiline: true,
                placeholder: Some(placeholder),
                label: None,
                text: "refactor the parser so errors carry line numbers",
                language: &language,
                clipboard_kind: None,
                previous_category: None,
                user_override: None,
            });
            assert_eq!(a.kind, IntentKind::Prompt, "{placeholder}");
        }
    }

    /// Labels as the IDEs expose them: Claude Code's prompt box has only a CSS
    /// placeholder, and VS Code's chat inputs are editors with an aria-label.
    #[test]
    fn ide_chat_boxes_are_prompts_by_their_label_and_editors_stay_code() {
        let text = "refactor the parser so errors carry line numbers";
        let language = detect(text);
        let classify_label = |label: &str, placeholder: Option<&str>| {
            classify(&IntentSignals {
                category: AppCategory::Ide,
                role: Some(InputRole::TextArea),
                is_multiline: true,
                placeholder,
                label: Some(label),
                text,
                language: &language,
                clipboard_kind: None,
                previous_category: None,
                user_override: None,
            })
        };
        for label in [
            "Message input",
            "Chat input. Press Enter to send out the request. Use ⌥F1 for Chat Accessibility Help.",
            "Inline Chat Input, Use ⌥F1 for Inline Chat Accessibility Help.",
            "Ask a side question",
        ] {
            assert_eq!(classify_label(label, None).kind, IntentKind::Prompt, "{label}");
        }
        assert_eq!(classify_label("Editor content", None).kind, IntentKind::Code);
        let commit = classify_label("Source Control Input", Some("Message (⌘⏎ to commit on \"main\")"));
        assert_ne!(commit.kind, IntentKind::Prompt);
    }

    #[test]
    fn a_chat_input_label_outside_an_ide_is_not_a_prompt_signal() {
        let a = run(Case {
            category: AppCategory::Chat,
            role: Some(InputRole::TextArea),
            multiline: true,
            placeholder: Some("Message input"),
            text: "running 10 min late, start without me",
        });
        assert_eq!(a.kind, IntentKind::Conversation);
    }

    #[test]
    fn chat_app_messages_are_conversations() {
        let a = run(Case {
            category: AppCategory::Chat,
            role: Some(InputRole::TextArea),
            multiline: true,
            placeholder: Some("Message #deployments"),
            text: "bhai kal 10 baje milte hai",
        });
        assert_eq!(a.kind, IntentKind::Conversation);
        assert_eq!(a.subtype, Some(IntentSubtype::CasualMessage));
        assert!(a.confidence > 0.7);
    }

    #[test]
    fn imperative_requests_in_chat_apps_stay_conversations() {
        let a = run(Case {
            category: AppCategory::Chat,
            role: Some(InputRole::TextArea),
            multiline: true,
            placeholder: None,
            text: "Can you send me the deployment report before the client meeting?",
        });
        assert_eq!(a.kind, IntentKind::Conversation);
        assert_eq!(a.subtype, Some(IntentSubtype::ProfessionalMessage));
    }

    #[test]
    fn email_composer() {
        let a = run(Case {
            category: AppCategory::Email,
            role: Some(InputRole::Document),
            multiline: true,
            placeholder: None,
            text: "Hi Asha,\n\nI wanted to inform you that the testing is completd.\n\nRegards,\nPrathamesh",
        });
        assert_eq!(a.kind, IntentKind::Conversation);
        assert_eq!(a.subtype, Some(IntentSubtype::Email));
    }

    #[test]
    fn ai_assistant_prompts() {
        let a = run(Case {
            category: AppCategory::AiAssistant,
            role: Some(InputRole::TextArea),
            multiline: true,
            placeholder: Some("Ask anything"),
            text: "fix this code it is giving error",
        });
        assert_eq!(a.kind, IntentKind::Prompt);
        assert_eq!(a.subtype, Some(IntentSubtype::Coding));
        assert!(a.confidence > 0.8);
    }

    #[test]
    fn prompt_in_ide_chat_panel() {
        let a = run(Case {
            category: AppCategory::Ide,
            role: Some(InputRole::TextArea),
            multiline: true,
            placeholder: Some("Ask Copilot or type / for commands"),
            text: "why does this test fail only on windows",
        });
        assert_eq!(a.kind, IntentKind::Prompt);
    }

    #[test]
    fn prompt_subtypes() {
        let research = run(Case {
            category: AppCategory::AiAssistant,
            role: None,
            multiline: true,
            placeholder: None,
            text: "compare the latest studies on remote work productivity and cite sources",
        });
        assert_eq!(research.subtype, Some(IntentSubtype::Research));
        let reasoning = run(Case {
            category: AppCategory::AiAssistant,
            role: None,
            multiline: true,
            placeholder: None,
            text: "solve this probability puzzle step by step and prove the answer",
        });
        assert_eq!(reasoning.subtype, Some(IntentSubtype::Reasoning));
        let general = run(Case {
            category: AppCategory::AiAssistant,
            role: None,
            multiline: true,
            placeholder: None,
            text: "write a short poem about monsoon evenings",
        });
        assert_eq!(general.subtype, Some(IntentSubtype::General));
    }

    #[test]
    fn hinglish_prompt_in_ai_app_is_a_prompt() {
        let a = run(Case {
            category: AppCategory::AiAssistant,
            role: Some(InputRole::TextArea),
            multiline: true,
            placeholder: None,
            text: "bhai ek python script likh do jo csv ko json me convert kare",
        });
        assert_eq!(a.kind, IntentKind::Prompt);
    }

    #[test]
    fn terminal_commands() {
        let a = run(Case {
            category: AppCategory::Terminal,
            role: Some(InputRole::Terminal),
            multiline: false,
            placeholder: None,
            text: "git push origin main --force-with-lease",
        });
        assert_eq!(a.kind, IntentKind::Command);
    }

    #[test]
    fn code_in_editor() {
        let a = run(Case {
            category: AppCategory::Ide,
            role: Some(InputRole::TextArea),
            multiline: true,
            placeholder: None,
            text: "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}",
        });
        assert_eq!(a.kind, IntentKind::Code);
    }

    #[test]
    fn notes_app() {
        let a = run(Case {
            category: AppCategory::Notes,
            role: Some(InputRole::Document),
            multiline: true,
            placeholder: None,
            text: "Meeting notes: discussed the Q3 roadmap and hiring plan.",
        });
        assert_eq!(a.kind, IntentKind::Note);
    }

    #[test]
    fn search_fields() {
        let a = run(Case {
            category: AppCategory::Browser,
            role: Some(InputRole::SearchField),
            multiline: false,
            placeholder: Some("Search"),
            text: "rust async traits",
        });
        assert_eq!(a.kind, IntentKind::Search);
        let launcher = run(Case {
            category: AppCategory::Launcher,
            role: None,
            multiline: false,
            placeholder: None,
            text: "calc",
        });
        assert_eq!(launcher.kind, IntentKind::Search);
    }

    #[test]
    fn form_fields() {
        let a = run(Case {
            category: AppCategory::Browser,
            role: Some(InputRole::TextField),
            multiline: false,
            placeholder: Some("Company name"),
            text: "Acme",
        });
        assert_eq!(a.kind, IntentKind::Form);
    }

    #[test]
    fn generic_browser_text_is_uncertain() {
        let a = run(Case {
            category: AppCategory::Browser,
            role: Some(InputRole::TextArea),
            multiline: true,
            placeholder: None,
            text: "The quarterly numbers look better than expected overall",
        });
        assert!(a.confidence < AI_CLASSIFICATION_THRESHOLD, "{a:?}");
        assert!(needs_ai_classification(&a, "The quarterly numbers look better than expected overall"));
    }

    #[test]
    fn user_override_wins() {
        let language = detect("anything");
        let a = classify(&IntentSignals {
            category: AppCategory::Chat,
            role: None,
            is_multiline: true,
            placeholder: None,
            label: None,
            text: "anything",
            language: &language,
            clipboard_kind: None,
            previous_category: None,
            user_override: Some(IntentKind::Prompt),
        });
        assert_eq!(a.kind, IntentKind::Prompt);
        assert_eq!(a.source, IntentSource::UserRule);
    }

    #[test]
    fn parses_model_output() {
        let a = parse_ai_classification(r#"{"kind": "conversation", "subtype": "chat_message", "confidence": 0.98}"#)
            .unwrap();
        assert_eq!(a.kind, IntentKind::Conversation);
        assert_eq!(a.subtype, None, "unknown subtype is dropped");
        let b = parse_ai_classification("Sure! {\"kind\":\"prompt\",\"subtype\":\"coding\",\"confidence\":0.95} done")
            .unwrap();
        assert_eq!(b.kind, IntentKind::Prompt);
        assert_eq!(b.subtype, Some(IntentSubtype::Coding));
        let c = parse_ai_classification(r#"{"kind":"prompt","subtype":"email"}"#).unwrap();
        assert_eq!(c.subtype, None, "subtype must belong to kind");
        assert!(parse_ai_classification("no json here").is_none());
        assert_eq!(parse_ai_classification(r#"{"kind":"weird"}"#).unwrap().kind, IntentKind::Unknown);
    }
}
