//! Contextual suggestions derived from recent activity.
//!
//! Example: the user copies an email in Gmail, switches to their IDE and focuses
//! the AI prompt box. Mote infers the copied email may be relevant and offers
//! "Use copied content" with actions such as *Create coding task* or
//! *Summarize*. The rules are deterministic, and nothing is sent to the
//! provider unless the user picks an action.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::clipboard::ClipboardKind;
use super::manager::ContextWindow;
use crate::intent::apps::AppCategory;
use crate::intent::IntentKind;

/// Something Mote can do with copied content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ContextAction {
    CreateCodingTask,
    AnalyzeIssue,
    DebugError,
    ExplainCode,
    Summarize,
    DraftResponse,
    CreatePrompt,
}

impl ContextAction {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::CreateCodingTask => "Create coding task",
            Self::AnalyzeIssue => "Analyze issue",
            Self::DebugError => "Debug this error",
            Self::ExplainCode => "Explain code",
            Self::Summarize => "Summarize",
            Self::DraftResponse => "Draft response",
            Self::CreatePrompt => "Create prompt",
        }
    }
}

/// An offer to use recently copied content in the current input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ContextSuggestion {
    pub source_app: String,
    pub clipboard_kind: ClipboardKind,
    pub char_count: u32,
    pub age_secs: u32,
    pub actions: Vec<ContextAction>,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub captured_at: DateTime<Utc>,
}

/// Copied content older than this is not offered.
pub const MAX_COPY_AGE_SECS: i64 = 180;
/// Content shorter than this is not worth a task.
pub const MIN_CONTENT_CHARS: usize = 40;
/// Offer only while the user has typed little into the current field.
pub const MAX_EXISTING_TEXT_CHARS: usize = 60;

/// Decides whether to offer the copied content for the current input.
pub fn suggest(
    window: &ContextWindow,
    intent: IntentKind,
    current_text_chars: usize,
    now: DateTime<Utc>,
) -> Option<ContextSuggestion> {
    let clip = window.clipboard.as_ref()?;
    let active = window.active_app.as_ref()?;
    let source = clip.source_app.as_ref()?;
    if source.id == active.id || !clip.kind.is_substantial() || clip.char_count < MIN_CONTENT_CHARS {
        return None;
    }
    let age = (now - clip.captured_at).num_seconds();
    if !(0..=MAX_COPY_AGE_SECS).contains(&age) || current_text_chars > MAX_EXISTING_TEXT_CHARS {
        return None;
    }
    let coding_target = matches!(window.category, AppCategory::Ide | AppCategory::AiAssistant);
    let actions = match (intent, clip.kind) {
        (IntentKind::Prompt, ClipboardKind::StackTrace) => {
            vec![ContextAction::DebugError, ContextAction::AnalyzeIssue, ContextAction::CreatePrompt]
        }
        (IntentKind::Prompt, ClipboardKind::Code | ClipboardKind::Json) => {
            vec![ContextAction::ExplainCode, ContextAction::AnalyzeIssue, ContextAction::CreatePrompt]
        }
        (IntentKind::Prompt, _) => {
            let mut actions = Vec::new();
            if coding_target {
                actions.push(ContextAction::CreateCodingTask);
            }
            actions.extend([ContextAction::AnalyzeIssue, ContextAction::Summarize, ContextAction::CreatePrompt]);
            actions
        }
        (IntentKind::Conversation, _) => vec![ContextAction::DraftResponse, ContextAction::Summarize],
        (IntentKind::Note, _) => vec![ContextAction::Summarize, ContextAction::CreatePrompt],
        (IntentKind::Unknown, _) if coding_target => vec![ContextAction::CreateCodingTask, ContextAction::Summarize],
        _ => return None,
    };
    Some(ContextSuggestion {
        source_app: source.name.clone(),
        clipboard_kind: clip.kind,
        char_count: u32::try_from(clip.char_count).unwrap_or(u32::MAX),
        age_secs: u32::try_from(age).unwrap_or(0),
        actions,
        captured_at: clip.captured_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::manager::ContextManager;
    use crate::platform::{AppInfo, OsPlatform};
    use chrono::Duration as ChronoDuration;
    use std::time::Duration;

    const EMAIL: &str = "Hi Prathamesh,\n\nThe CSV export button throws an error for customers on Safari since the last release.\nCould you investigate?\n\nThanks,\nAsha";

    fn gmail_to_ide() -> (ContextManager, DateTime<Utc>) {
        let mut m = ContextManager::new(OsPlatform::Macos, Duration::from_secs(180));
        let t0 = Utc::now();
        let chrome = AppInfo::new("com.google.chrome", "Google Chrome");
        m.on_app_activated(chrome.clone(), Some("Inbox - Gmail".into()), t0);
        m.on_clipboard(EMAIL, Some(chrome), t0);
        m.on_app_activated(
            AppInfo::new("com.microsoft.vscode", "Visual Studio Code"),
            None,
            t0 + ChronoDuration::seconds(4),
        );
        (m, t0 + ChronoDuration::seconds(6))
    }

    #[test]
    fn email_copied_then_ide_prompt_offers_coding_task() {
        let (m, now) = gmail_to_ide();
        let s = suggest(&m.window(now), IntentKind::Prompt, 0, now).expect("suggestion");
        assert_eq!(s.source_app, "Google Chrome");
        assert_eq!(s.clipboard_kind, ClipboardKind::Email);
        assert_eq!(s.actions[0], ContextAction::CreateCodingTask);
        assert!(s.actions.contains(&ContextAction::Summarize));
    }

    #[test]
    fn conversation_targets_get_reply_actions() {
        let (m, now) = gmail_to_ide();
        let s = suggest(&m.window(now), IntentKind::Conversation, 0, now).unwrap();
        assert_eq!(s.actions, vec![ContextAction::DraftResponse, ContextAction::Summarize]);
    }

    #[test]
    fn stack_traces_offer_debugging() {
        let mut m = ContextManager::new(OsPlatform::Macos, Duration::from_secs(180));
        let t0 = Utc::now();
        let term = AppInfo::new("com.apple.terminal", "Terminal");
        m.on_app_activated(term.clone(), None, t0);
        m.on_clipboard(
            "TypeError: Cannot read properties of undefined (reading 'map')\n    at render (App.tsx:12:5)\n    at main (index.ts:3:1)",
            Some(term),
            t0,
        );
        m.on_app_activated(AppInfo::new("com.openai.chat", "ChatGPT"), None, t0);
        let s = suggest(&m.window(t0), IntentKind::Prompt, 0, t0).unwrap();
        assert_eq!(s.actions[0], ContextAction::DebugError);
    }

    #[test]
    fn no_suggestion_when_conditions_fail() {
        let (m, now) = gmail_to_ide();
        let w = m.window(now);
        assert!(suggest(&w, IntentKind::Prompt, 200, now).is_none(), "user already wrote a lot");
        assert!(suggest(&w, IntentKind::Command, 0, now).is_none(), "terminal commands");
        let later = now + ChronoDuration::seconds(MAX_COPY_AGE_SECS + 1);
        assert!(suggest(&w, IntentKind::Prompt, 0, later).is_none(), "copy too old");

        // Copied in the same app it is pasted into.
        let mut m = ContextManager::new(OsPlatform::Macos, Duration::from_secs(180));
        let code = AppInfo::new("com.microsoft.vscode", "Visual Studio Code");
        m.on_app_activated(code.clone(), None, now);
        m.on_clipboard(EMAIL, Some(code), now);
        assert!(suggest(&m.window(now), IntentKind::Prompt, 0, now).is_none());
    }
}
