//! User settings, their defaults and validation.
//!
//! Settings are stored as one JSON document. Every struct uses
//! `#[serde(default)]`, so documents written by older versions load with new
//! fields defaulted, and unknown fields from newer versions are ignored.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::prompts::EnhanceStyle;

/// Current settings schema version.
pub const SETTINGS_VERSION: u32 = 1;

/// Default Groq endpoint (OpenAI-compatible).
pub const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";

/// Default model assignments, chosen from Groq's live catalogue (October 2026):
/// Qwen 3.8 27B with reasoning disabled answers in ~350 ms, which suits
/// completion, classification and short writing fixes; GPT-OSS 120B gives the
/// best quality for prompt enhancement; GPT-OSS 20B is the production-tier
/// fallback if a preview model is withdrawn.
pub mod default_models {
    pub const COMPLETION: &str = "qwen/qwen3.8-27b";
    pub const CLASSIFICATION: &str = "qwen/qwen3.8-27b";
    pub const WRITING: &str = "qwen/qwen3.8-27b";
    pub const REASONING: &str = "openai/gpt-oss-120b";
    pub const FALLBACK: &str = "openai/gpt-oss-20b";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub general: GeneralSettings,
    pub provider: ProviderSettings,
    pub completion: CompletionSettings,
    pub writing: WritingSettings,
    pub prompts: PromptSettings,
    pub context: ContextSettings,
    pub privacy: PrivacySettings,
    pub usage: UsageSettings,
    pub keyboard: KeyboardSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            general: GeneralSettings::default(),
            provider: ProviderSettings::default(),
            completion: CompletionSettings::default(),
            writing: WritingSettings::default(),
            prompts: PromptSettings::default(),
            context: ContextSettings::default(),
            privacy: PrivacySettings::default(),
            usage: UsageSettings::default(),
            keyboard: KeyboardSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralSettings {
    /// Master switch for all ambient assistance.
    pub assistance_enabled: bool,
    pub launch_at_login: bool,
    pub onboarding_completed: bool,
    /// When set and in the future, Mote observes nothing until this time.
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub paused_until: Option<DateTime<Utc>>,
    pub theme: Theme,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            assistance_enabled: true,
            launch_at_login: false,
            onboarding_completed: false,
            paused_until: None,
            theme: Theme::System,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    Groq,
}

impl ProviderKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Groq => "groq",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderSettings {
    pub active: ProviderKind,
    pub groq: GroqSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct GroqSettings {
    pub base_url: String,
    pub request_timeout_ms: u32,
    pub models: ModelAssignments,
}

impl Default for GroqSettings {
    fn default() -> Self {
        Self { base_url: GROQ_BASE_URL.to_string(), request_timeout_ms: 20_000, models: ModelAssignments::default() }
    }
}

/// Which model serves each logical role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct ModelAssignments {
    pub completion: String,
    pub classification: String,
    pub writing: String,
    pub reasoning: String,
    /// Used when a role's model is unavailable (removed or not accessible).
    pub fallback: String,
}

impl Default for ModelAssignments {
    fn default() -> Self {
        Self {
            completion: default_models::COMPLETION.into(),
            classification: default_models::CLASSIFICATION.into(),
            writing: default_models::WRITING.into(),
            reasoning: default_models::REASONING.into(),
            fallback: default_models::FALLBACK.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct CompletionSettings {
    pub enabled: bool,
    /// Pause after the last change before a completion is requested.
    pub debounce_ms: u32,
    /// Minimum text length before the caret.
    pub min_chars: u32,
    /// Upper bound on suggestion length.
    pub max_words: u32,
    /// Minimum time between two completion requests.
    pub min_interval_ms: u32,
    pub in_conversations: bool,
    pub in_prompts: bool,
    pub in_notes: bool,
    pub in_unknown: bool,
}

impl Default for CompletionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_ms: 450,
            min_chars: 12,
            max_words: 12,
            min_interval_ms: 1_200,
            in_conversations: true,
            in_prompts: true,
            in_notes: true,
            in_unknown: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct WritingSettings {
    pub enabled: bool,
    /// Local, offline spelling suggestions (no tokens).
    pub spelling: bool,
    /// Ask the writing model to fix grammar when a sentence looks wrong.
    pub ai_grammar: bool,
    /// Words the user told Mote to accept (lowercase).
    pub ignored_words: Vec<String>,
}

impl Default for WritingSettings {
    fn default() -> Self {
        Self { enabled: true, spelling: true, ai_grammar: true, ignored_words: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct PromptSettings {
    pub enhancement_enabled: bool,
    /// Show a small "Enhance prompt" hint when Mote detects a prompt; Tab on it
    /// enhances the prompt in place.
    pub show_hint: bool,
    /// The style used when enhancing from the hint.
    pub default_style: EnhanceStyle,
}

impl Default for PromptSettings {
    fn default() -> Self {
        Self { enhancement_enabled: true, show_hint: true, default_style: EnhanceStyle::Improve }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct ContextSettings {
    /// Suggest using recently copied content when it looks relevant.
    pub contextual_suggestions: bool,
    /// How long copied text stays available to Mote (memory only).
    pub clipboard_ttl_secs: u32,
    /// Allow a model call when local signals cannot classify the context.
    pub ai_classification: bool,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self { contextual_suggestions: true, clipboard_ttl_secs: 180, ai_classification: true }
    }
}

/// How long context metadata is kept on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RetentionPeriod {
    /// Keep context in memory only.
    Off,
    OneHour,
    #[default]
    OneDay,
    OneWeek,
}

impl RetentionPeriod {
    pub fn as_duration(self) -> Option<chrono::Duration> {
        match self {
            Self::Off => None,
            Self::OneHour => Some(chrono::Duration::hours(1)),
            Self::OneDay => Some(chrono::Duration::days(1)),
            Self::OneWeek => Some(chrono::Duration::days(7)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct PrivacySettings {
    /// Allow sending text to the cloud AI provider at all.
    pub cloud_ai_enabled: bool,
    pub observe_applications: bool,
    pub observe_text: bool,
    pub observe_clipboard: bool,
    pub context_retention: RetentionPeriod,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            cloud_ai_enabled: true,
            observe_applications: true,
            observe_text: true,
            observe_clipboard: true,
            context_retention: RetentionPeriod::OneDay,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct UsageSettings {
    /// Record local usage statistics (metadata only).
    pub analytics_enabled: bool,
    /// Days of usage history to keep.
    pub retention_days: u32,
}

impl Default for UsageSettings {
    fn default() -> Self {
        Self { analytics_enabled: true, retention_days: 180 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", default)]
pub struct KeyboardSettings {
    /// Opens the command palette from anywhere.
    pub command_palette: String,
    /// Shows the next suggestion while one is visible.
    pub next_suggestion: String,
    /// Shows the previous suggestion while one is visible.
    pub previous_suggestion: String,
}

impl Default for KeyboardSettings {
    fn default() -> Self {
        Self {
            command_palette: "CommandOrControl+Shift+Space".into(),
            next_suggestion: "Alt+BracketRight".into(),
            previous_suggestion: "Alt+BracketLeft".into(),
        }
    }
}

/// A validation failure for one settings field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingsError {
    pub field: String,
    pub message: String,
}

impl Settings {
    /// Validates every field; returns all problems at once.
    pub fn validate(&self) -> Result<(), Vec<SettingsError>> {
        let mut errors = Vec::new();
        let mut check = |ok: bool, field: &str, message: &str| {
            if !ok {
                errors.push(SettingsError { field: field.to_string(), message: message.to_string() });
            }
        };
        let groq = &self.provider.groq;
        check(
            is_allowed_base_url(&groq.base_url),
            "provider.groq.baseUrl",
            "Must be an https:// URL without a username, password or query (http is allowed only for localhost).",
        );
        check(
            (1_000..=120_000).contains(&groq.request_timeout_ms),
            "provider.groq.requestTimeoutMs",
            "Must be between 1 and 120 seconds.",
        );
        for (field, model) in [
            ("provider.groq.models.completion", &groq.models.completion),
            ("provider.groq.models.classification", &groq.models.classification),
            ("provider.groq.models.writing", &groq.models.writing),
            ("provider.groq.models.reasoning", &groq.models.reasoning),
            ("provider.groq.models.fallback", &groq.models.fallback),
        ] {
            check(is_valid_model_id(model), field, "Model IDs contain only letters, digits and . _ - / : (max 128).");
        }
        let c = &self.completion;
        check((150..=3_000).contains(&c.debounce_ms), "completion.debounceMs", "Must be between 150 and 3000 ms.");
        check((1..=500).contains(&c.min_chars), "completion.minChars", "Must be between 1 and 500.");
        check((1..=40).contains(&c.max_words), "completion.maxWords", "Must be between 1 and 40.");
        check(c.min_interval_ms <= 30_000, "completion.minIntervalMs", "Must be at most 30 seconds.");
        check(self.writing.ignored_words.len() <= 2_000, "writing.ignoredWords", "At most 2000 words.");
        check(
            self.writing.ignored_words.iter().all(|w| !w.is_empty() && w.chars().count() <= 64),
            "writing.ignoredWords",
            "Words must be 1-64 characters.",
        );
        check(
            (10..=3_600).contains(&self.context.clipboard_ttl_secs),
            "context.clipboardTtlSecs",
            "Must be between 10 seconds and 1 hour.",
        );
        check(
            (7..=3_650).contains(&self.usage.retention_days),
            "usage.retentionDays",
            "Must be between 7 and 3650 days.",
        );
        let shortcuts = [
            ("keyboard.commandPalette", "the command palette", &self.keyboard.command_palette),
            ("keyboard.nextSuggestion", "Next suggestion", &self.keyboard.next_suggestion),
            ("keyboard.previousSuggestion", "Previous suggestion", &self.keyboard.previous_suggestion),
        ];
        let mut assigned: Vec<(Accelerator, &str)> = Vec::new();
        for (field, label, value) in shortcuts {
            match parse_accelerator(value) {
                Ok(accelerator) => {
                    let accelerator = accelerator.canonical();
                    if let Some((_, owner)) = assigned.iter().find(|(a, _)| *a == accelerator) {
                        check(false, field, &format!("Already used for {owner}."));
                    } else {
                        assigned.push((accelerator, label));
                    }
                }
                Err(message) => check(false, field, &message),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Whether the user has paused Mote at `now`.
    pub fn is_paused(&self, now: DateTime<Utc>) -> bool {
        self.general.paused_until.is_some_and(|until| until > now)
    }
}

/// An `https://` URL, or `http://` to a loopback address (a local proxy or
/// model server). Credentials, queries and fragments are never accepted, so a
/// URL such as `http://localhost:x@example.com` cannot pass as local.
fn is_allowed_base_url(raw: &str) -> bool {
    if raw.len() > 256 || raw.chars().any(char::is_whitespace) {
        return false;
    }
    let Ok(url) = url::Url::parse(raw) else { return false };
    if !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return false;
    }
    match (url.scheme(), url.host()) {
        ("https", Some(_)) => true,
        ("http", Some(url::Host::Domain(host))) => host == "localhost",
        ("http", Some(url::Host::Ipv4(ip))) => ip.is_loopback(),
        ("http", Some(url::Host::Ipv6(ip))) => ip.is_loopback(),
        _ => false,
    }
}

/// Whether `id` looks like a provider model identifier.
pub fn is_valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':'))
}

/// A parsed keyboard accelerator such as `CommandOrControl+Shift+Space`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accelerator {
    pub modifiers: Vec<String>,
    pub key: String,
}

impl Accelerator {
    /// Aliases resolved and modifiers sorted, for comparing two shortcuts.
    pub fn canonical(&self) -> Self {
        let mut modifiers: Vec<String> = self
            .modifiers
            .iter()
            .map(|m| {
                match m.as_str() {
                    "cmdorctrl" => "commandorcontrol",
                    "ctrl" => "control",
                    "option" => "alt",
                    "command" | "cmd" | "meta" => "super",
                    other => other,
                }
                .to_string()
            })
            .collect();
        modifiers.sort();
        modifiers.dedup();
        Self { modifiers, key: self.key.to_lowercase() }
    }
}

const MODIFIERS: &[&str] =
    &["commandorcontrol", "cmdorctrl", "command", "cmd", "super", "meta", "control", "ctrl", "alt", "option", "shift"];

/// Parses an accelerator string in Tauri's format.
pub fn parse_accelerator(s: &str) -> Result<Accelerator, String> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err("Shortcut is empty or malformed.".into());
    }
    let (key, modifiers) = parts.split_last().ok_or("Shortcut is empty.")?;
    let mut seen = Vec::new();
    for m in modifiers {
        let lower = m.to_lowercase();
        if !MODIFIERS.contains(&lower.as_str()) {
            return Err(format!("Unknown modifier \"{m}\"."));
        }
        if seen.contains(&lower) {
            return Err(format!("Modifier \"{m}\" is repeated."));
        }
        seen.push(lower);
    }
    if !is_valid_key(key) {
        return Err(format!("Unsupported key \"{key}\"."));
    }
    if seen.is_empty() {
        return Err("Global shortcuts need at least one modifier.".into());
    }
    if seen.iter().all(|m| m == "shift") {
        return Err("Add Ctrl, Alt or Cmd: with Shift alone the shortcut would fire while typing.".into());
    }
    Ok(Accelerator { modifiers: seen, key: key.to_string() })
}

fn is_valid_key(key: &str) -> bool {
    let k = key.to_lowercase();
    let named = [
        "space",
        "tab",
        "enter",
        "escape",
        "backspace",
        "delete",
        "up",
        "down",
        "left",
        "right",
        "home",
        "end",
        "pageup",
        "pagedown",
        "bracketleft",
        "bracketright",
        "comma",
        "period",
        "slash",
        "backslash",
        "semicolon",
        "quote",
        "minus",
        "equal",
        "backquote",
        "[",
        "]",
        ",",
        ".",
        "/",
        ";",
        "'",
        "-",
        "=",
        "`",
    ];
    if named.contains(&k.as_str()) {
        return true;
    }
    if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) {
        return true;
    }
    if let Some(n) = k.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&n);
    }
    k.strip_prefix("key").is_some_and(|c| c.len() == 1 && c.chars().all(|c| c.is_ascii_alphabetic()))
        || k.strip_prefix("digit").is_some_and(|c| c.len() == 1 && c.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert_eq!(Settings::default().validate(), Ok(()));
    }

    #[test]
    fn json_roundtrip_and_forward_compatibility() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        // Missing sections and unknown fields are tolerated.
        let partial: Settings = serde_json::from_str(r#"{"completion":{"enabled":false},"futureField":1}"#).unwrap();
        assert!(!partial.completion.enabled);
        assert_eq!(partial.completion.debounce_ms, 450);
        assert_eq!(partial.provider.groq.base_url, GROQ_BASE_URL);
    }

    #[test]
    fn rejects_unsafe_base_urls() {
        let mut s = Settings::default();
        for bad in [
            "http://api.groq.com/openai/v1",
            "ftp://x",
            "",
            "https://",
            "https://exa mple.com",
            "http://localhost:x@evil.example/v1",
            "http://localhost@evil.example/v1",
            "https://user:pass@api.groq.com/openai/v1",
            "http://localhost.evil.example/v1",
            "https://api.groq.com/openai/v1?key=1",
        ] {
            s.provider.groq.base_url = bad.into();
            assert!(s.validate().is_err(), "{bad}");
        }
        for good in [
            "https://api.groq.com/openai/v1",
            "http://localhost:8080/v1",
            "http://127.0.0.1:11434/v1",
            "http://[::1]:8080/v1",
            "https://proxy.internal.example/groq/v1",
        ] {
            s.provider.groq.base_url = good.into();
            assert_eq!(s.validate(), Ok(()), "{good}");
        }
    }

    #[test]
    fn rejects_out_of_range_values() {
        let mut s = Settings::default();
        s.completion.debounce_ms = 10;
        s.completion.max_words = 0;
        s.usage.retention_days = 1;
        s.provider.groq.models.completion = "bad model id!".into();
        let errors = s.validate().unwrap_err();
        let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"completion.debounceMs"));
        assert!(fields.contains(&"completion.maxWords"));
        assert!(fields.contains(&"usage.retentionDays"));
        assert!(fields.contains(&"provider.groq.models.completion"));
    }

    #[test]
    fn accelerators() {
        assert!(parse_accelerator("CommandOrControl+Shift+Space").is_ok());
        assert!(parse_accelerator("Alt+BracketRight").is_ok());
        assert!(parse_accelerator("Ctrl+Alt+F12").is_ok());
        assert!(parse_accelerator("Space").is_err(), "needs a modifier");
        assert!(parse_accelerator("Hyper+K").is_err());
        assert!(parse_accelerator("Ctrl+Ctrl+K").is_err());
        assert!(parse_accelerator("Ctrl+").is_err());
        assert!(parse_accelerator("Ctrl+F25").is_err());
        assert!(parse_accelerator("Shift+K").is_err(), "Shift alone fires while typing capitals");
        assert!(parse_accelerator("Shift+Alt+K").is_ok());
    }

    #[test]
    fn shortcuts_must_be_distinct() {
        let mut s = Settings::default();
        s.keyboard.previous_suggestion = s.keyboard.next_suggestion.clone();
        let errors = s.validate().unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "keyboard.previousSuggestion");
        // Aliases and modifier order don't make a shortcut different.
        s.keyboard.previous_suggestion = "Alt+BracketLeft".into();
        s.keyboard.next_suggestion = "Shift+CmdOrCtrl+Space".into();
        let errors = s.validate().unwrap_err();
        assert_eq!(errors[0].field, "keyboard.nextSuggestion");
        assert!(errors[0].message.contains("command palette"), "{}", errors[0].message);
    }

    #[test]
    fn pause_state() {
        let mut s = Settings::default();
        let now = Utc::now();
        assert!(!s.is_paused(now));
        s.general.paused_until = Some(now + chrono::Duration::minutes(5));
        assert!(s.is_paused(now));
    }

    #[test]
    fn model_ids() {
        assert!(is_valid_model_id("openai/gpt-oss-20b"));
        assert!(is_valid_model_id("qwen/qwen3.8-27b"));
        assert!(!is_valid_model_id(""));
        assert!(!is_valid_model_id("a b"));
    }
}
