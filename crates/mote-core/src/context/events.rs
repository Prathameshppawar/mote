//! Structured context events.
//!
//! Events describe *what happened*, never *what was written*: application
//! switches, focus changes, clipboard metadata (kind and length) and assistance
//! outcomes. They are safe to persist for the user's retention period and to
//! show in the activity log.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::clipboard::ClipboardKind;
use crate::intent::apps::AppCategory;
use crate::intent::IntentKind;
use crate::platform::InputRole;
use crate::providers::types::Feature;

/// What happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextEventKind {
    /// The frontmost application changed.
    ApplicationChanged { from: Option<String>, to: String, category: AppCategory },
    /// A text input received focus.
    InputFocused { app: String, role: InputRole },
    /// The writing context was classified.
    IntentClassified { app: String, kind: IntentKind, confidence: f32 },
    /// The clipboard changed. Content is never recorded.
    ClipboardChanged { source_app: Option<String>, kind: ClipboardKind, char_count: u32 },
    /// Mote showed a suggestion.
    SuggestionShown { feature: Feature },
    /// The user accepted a suggestion.
    SuggestionAccepted { feature: Feature },
    /// The user dismissed a suggestion.
    SuggestionDismissed { feature: Feature },
    /// Mote was paused by the user.
    Paused { minutes: Option<u32> },
    /// Mote resumed.
    Resumed,
}

impl ContextEventKind {
    /// Stable type name, as serialized in the `type` field.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::ApplicationChanged { .. } => "application_changed",
            Self::InputFocused { .. } => "input_focused",
            Self::IntentClassified { .. } => "intent_classified",
            Self::ClipboardChanged { .. } => "clipboard_changed",
            Self::SuggestionShown { .. } => "suggestion_shown",
            Self::SuggestionAccepted { .. } => "suggestion_accepted",
            Self::SuggestionDismissed { .. } => "suggestion_dismissed",
            Self::Paused { .. } => "paused",
            Self::Resumed => "resumed",
        }
    }
}

/// A timestamped context event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ContextEvent {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub timestamp: DateTime<Utc>,
    /// `macos`, `windows` or `mote`.
    pub source: String,
    #[serde(flatten)]
    pub kind: ContextEventKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_in_the_documented_shape() {
        let event = ContextEvent {
            timestamp: "2026-10-07T10:31:00Z".parse().unwrap(),
            source: "macos".into(),
            kind: ContextEventKind::ApplicationChanged {
                from: Some("Google Chrome".into()),
                to: "Visual Studio Code".into(),
                category: AppCategory::Ide,
            },
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "application_changed");
        assert_eq!(json["source"], "macos");
        assert_eq!(json["from"], "Google Chrome");
        assert_eq!(json["to"], "Visual Studio Code");
        assert_eq!(json["timestamp"], "2026-10-07T10:31:00Z");
        let back: ContextEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn clipboard_events_carry_no_content() {
        let kind = ContextEventKind::ClipboardChanged {
            source_app: Some("Mail".into()),
            kind: ClipboardKind::Email,
            char_count: 512,
        };
        let json = serde_json::to_value(&kind).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 4, "type, source_app, kind, char_count only");
        assert_eq!(kind.type_name(), "clipboard_changed");
    }
}
