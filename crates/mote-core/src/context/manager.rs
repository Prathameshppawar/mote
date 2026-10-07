//! The context manager keeps a short-lived, in-memory picture of what the user
//! is doing and emits structured events for it.
//!
//! Clipboard text and window titles live here, in memory, for a bounded time
//! (the clipboard TTL) and are dropped on pause, exclusion or "clear context".
//! Only metadata leaves this module as [`ContextEvent`]s.

use std::collections::VecDeque;
use std::time::Duration;

use chrono::{DateTime, Utc};

use super::clipboard::{classify, ClipboardKind};
use super::events::{ContextEvent, ContextEventKind};
use crate::intent::apps::{categorize, AppCategory};
use crate::platform::{AppInfo, FocusedInput, InputRole, OsPlatform};
use crate::text::head_chars;

/// Maximum clipboard text kept in memory.
pub const CLIPBOARD_MAX_CHARS: usize = 8_000;
const MAX_EVENTS: usize = 200;
const MAX_TRANSITIONS: usize = 20;
const EVENT_WINDOW: Duration = Duration::from_secs(15 * 60);

/// Recently copied text, in memory only.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardSnapshot {
    pub text: String,
    pub kind: ClipboardKind,
    pub char_count: usize,
    pub source_app: Option<AppInfo>,
    pub captured_at: DateTime<Utc>,
}

/// Clipboard metadata exposed to classifiers (no text).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardMeta {
    pub kind: ClipboardKind,
    pub char_count: usize,
    pub source_app: Option<AppInfo>,
    pub captured_at: DateTime<Utc>,
}

/// An application switch.
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub from: AppInfo,
    pub to: AppInfo,
    pub at: DateTime<Utc>,
}

/// The focused input, summarized.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusSummary {
    pub app: AppInfo,
    pub role: InputRole,
    pub element_key: u64,
    pub focused_at: DateTime<Utc>,
}

/// A snapshot of recent context for the intent and assistance engines.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextWindow {
    pub active_app: Option<AppInfo>,
    pub category: AppCategory,
    pub previous_app: Option<AppInfo>,
    pub previous_category: Option<AppCategory>,
    pub last_transition: Option<Transition>,
    pub clipboard: Option<ClipboardMeta>,
    pub focus: Option<FocusSummary>,
}

pub struct ContextManager {
    source: OsPlatform,
    clipboard_ttl: Duration,
    active_app: Option<AppInfo>,
    window_title: Option<String>,
    category: AppCategory,
    focus: Option<FocusSummary>,
    clipboard: Option<ClipboardSnapshot>,
    transitions: VecDeque<Transition>,
    events: VecDeque<ContextEvent>,
}

impl ContextManager {
    pub fn new(source: OsPlatform, clipboard_ttl: Duration) -> Self {
        Self {
            source,
            clipboard_ttl,
            active_app: None,
            window_title: None,
            category: AppCategory::Other,
            focus: None,
            clipboard: None,
            transitions: VecDeque::new(),
            events: VecDeque::new(),
        }
    }

    pub fn set_clipboard_ttl(&mut self, ttl: Duration) {
        self.clipboard_ttl = ttl;
    }

    fn push(&mut self, kind: ContextEventKind, now: DateTime<Utc>) -> ContextEvent {
        let event = ContextEvent { timestamp: now, source: self.source.as_str().to_string(), kind };
        self.events.push_back(event.clone());
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
        event
    }

    /// Records an event that originates from Mote itself (suggestions, pause).
    pub fn record(&mut self, kind: ContextEventKind, now: DateTime<Utc>) -> ContextEvent {
        let event = ContextEvent { timestamp: now, source: "mote".into(), kind };
        self.events.push_back(event.clone());
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
        event
    }

    /// The frontmost application (and its window title) changed or was refreshed.
    pub fn on_app_activated(&mut self, app: AppInfo, title: Option<String>, now: DateTime<Utc>) -> Vec<ContextEvent> {
        let mut events = Vec::new();
        let changed = self.active_app.as_ref().is_none_or(|current| current.id != app.id);
        let category = categorize(&app, title.as_deref());
        if changed {
            let from = self.active_app.take();
            if let Some(from_app) = &from {
                self.transitions.push_back(Transition { from: from_app.clone(), to: app.clone(), at: now });
                while self.transitions.len() > MAX_TRANSITIONS {
                    self.transitions.pop_front();
                }
            }
            events.push(self.push(
                ContextEventKind::ApplicationChanged { from: from.map(|a| a.name), to: app.name.clone(), category },
                now,
            ));
            self.focus = None;
        }
        self.category = category;
        self.window_title = title;
        self.active_app = Some(app);
        events
    }

    /// The current application is excluded or observation stopped: forget the
    /// focused input and window title, and do not record the application.
    pub fn on_unobservable(&mut self) {
        self.active_app = None;
        self.window_title = None;
        self.focus = None;
        self.category = AppCategory::Other;
    }

    /// Focus moved to `input` (or away from any text input).
    pub fn on_focus(&mut self, input: Option<&FocusedInput>, now: DateTime<Utc>) -> Vec<ContextEvent> {
        match input {
            Some(input) if !input.is_secure => {
                let same = self.focus.as_ref().is_some_and(|f| f.element_key == input.element_key);
                if same {
                    return Vec::new();
                }
                self.focus = Some(FocusSummary {
                    app: input.app.clone(),
                    role: input.role,
                    element_key: input.element_key,
                    focused_at: now,
                });
                vec![self.push(ContextEventKind::InputFocused { app: input.app.name.clone(), role: input.role }, now)]
            }
            _ => {
                self.focus = None;
                Vec::new()
            }
        }
    }

    /// The clipboard changed to `text` while `source_app` was frontmost.
    pub fn on_clipboard(&mut self, text: &str, source_app: Option<AppInfo>, now: DateTime<Utc>) -> Vec<ContextEvent> {
        let kept = head_chars(text, CLIPBOARD_MAX_CHARS).to_string();
        let kind = classify(&kept);
        let char_count = text.chars().count();
        self.clipboard =
            Some(ClipboardSnapshot { text: kept, kind, char_count, source_app: source_app.clone(), captured_at: now });
        vec![self.push(
            ContextEventKind::ClipboardChanged {
                source_app: source_app.map(|a| a.name),
                kind,
                char_count: u32::try_from(char_count).unwrap_or(u32::MAX),
            },
            now,
        )]
    }

    /// Forgets clipboard content (e.g. it was copied in an excluded app).
    pub fn forget_clipboard(&mut self) {
        self.clipboard = None;
    }

    /// The remembered clipboard, if still within its time-to-live.
    pub fn clipboard(&self, now: DateTime<Utc>) -> Option<&ClipboardSnapshot> {
        self.clipboard.as_ref().filter(|c| age(c.captured_at, now) <= self.clipboard_ttl)
    }

    /// In-memory window title of the active app (never persisted).
    pub fn window_title(&self) -> Option<&str> {
        self.window_title.as_deref()
    }

    pub fn active_app(&self) -> Option<&AppInfo> {
        self.active_app.as_ref()
    }

    pub fn category(&self) -> AppCategory {
        self.category
    }

    /// Drops expired clipboard content and old events.
    pub fn prune(&mut self, now: DateTime<Utc>) {
        if self.clipboard.as_ref().is_some_and(|c| age(c.captured_at, now) > self.clipboard_ttl) {
            self.clipboard = None;
        }
        while self.events.front().is_some_and(|e| age(e.timestamp, now) > EVENT_WINDOW) {
            self.events.pop_front();
        }
    }

    /// Clears everything held in memory.
    pub fn clear(&mut self) {
        self.clipboard = None;
        self.window_title = None;
        self.focus = None;
        self.transitions.clear();
        self.events.clear();
    }

    pub fn recent_events(&self) -> impl Iterator<Item = &ContextEvent> {
        self.events.iter()
    }

    /// A snapshot for classification and suggestions (no clipboard text).
    pub fn window(&self, now: DateTime<Utc>) -> ContextWindow {
        let last_transition = self.transitions.back().cloned();
        let previous_app = last_transition.as_ref().map(|t| t.from.clone());
        ContextWindow {
            active_app: self.active_app.clone(),
            category: self.category,
            previous_category: previous_app.as_ref().map(|a| categorize(a, None)),
            previous_app,
            last_transition,
            clipboard: self.clipboard(now).map(|c| ClipboardMeta {
                kind: c.kind,
                char_count: c.char_count,
                source_app: c.source_app.clone(),
                captured_at: c.captured_at,
            }),
            focus: self.focus.clone(),
        }
    }
}

fn age(at: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
    (now - at).to_std().unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    fn app(id: &str, name: &str) -> AppInfo {
        AppInfo::new(id, name)
    }

    fn input(app: AppInfo, key: u64) -> FocusedInput {
        FocusedInput {
            app,
            role: InputRole::TextArea,
            is_secure: false,
            is_multiline: true,
            is_web_content: false,
            placeholder: None,
            label: None,
            text_before_caret: String::new(),
            text_after_caret: String::new(),
            selected_text: None,
            total_length: None,
            caret_rect: None,
            element_key: key,
        }
    }

    #[test]
    fn records_application_transitions() {
        let mut m = ContextManager::new(OsPlatform::Macos, Duration::from_secs(180));
        let t0 = Utc::now();
        let first = m.on_app_activated(app("com.google.chrome", "Google Chrome"), Some("Gmail".into()), t0);
        assert_eq!(first.len(), 1);
        assert_eq!(m.category(), AppCategory::Email, "browser refined by title");
        // Same app again: no new event.
        assert!(m.on_app_activated(app("com.google.chrome", "Google Chrome"), Some("Gmail".into()), t0).is_empty());
        let events = m.on_app_activated(
            app("com.microsoft.vscode", "Visual Studio Code"),
            None,
            t0 + ChronoDuration::seconds(5),
        );
        match &events[0].kind {
            ContextEventKind::ApplicationChanged { from, to, category } => {
                assert_eq!(from.as_deref(), Some("Google Chrome"));
                assert_eq!(to, "Visual Studio Code");
                assert_eq!(*category, AppCategory::Ide);
            }
            other => panic!("unexpected {other:?}"),
        }
        let w = m.window(t0 + ChronoDuration::seconds(6));
        assert_eq!(w.previous_app.unwrap().name, "Google Chrome");
        assert_eq!(w.previous_category, Some(AppCategory::Browser));
        assert_eq!(events[0].source, "macos");
    }

    #[test]
    fn clipboard_is_kept_in_memory_for_its_ttl_only() {
        let mut m = ContextManager::new(OsPlatform::Windows, Duration::from_secs(60));
        let t0 = Utc::now();
        let events = m.on_clipboard(
            "Hi team,\nThe export fails on Safari.\nPlease check.\nThanks,\nAsha",
            Some(app("outlook.exe", "Outlook")),
            t0,
        );
        match &events[0].kind {
            ContextEventKind::ClipboardChanged { kind, char_count, source_app } => {
                assert_eq!(*kind, ClipboardKind::Email);
                assert!(*char_count > 20);
                assert_eq!(source_app.as_deref(), Some("Outlook"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(m.clipboard(t0 + ChronoDuration::seconds(30)).is_some());
        assert!(m.clipboard(t0 + ChronoDuration::seconds(61)).is_none());
        m.prune(t0 + ChronoDuration::seconds(61));
        assert!(m.window(t0).clipboard.is_none());
    }

    #[test]
    fn clipboard_text_is_bounded() {
        let mut m = ContextManager::new(OsPlatform::Macos, Duration::from_secs(60));
        let big = "a".repeat(CLIPBOARD_MAX_CHARS * 2);
        m.on_clipboard(&big, None, Utc::now());
        let c = m.clipboard(Utc::now()).unwrap();
        assert_eq!(c.text.chars().count(), CLIPBOARD_MAX_CHARS);
        assert_eq!(c.char_count, CLIPBOARD_MAX_CHARS * 2);
    }

    #[test]
    fn focus_events_fire_once_per_element_and_ignore_secure_fields() {
        let mut m = ContextManager::new(OsPlatform::Macos, Duration::from_secs(60));
        let now = Utc::now();
        let slack = app("com.tinyspeck.slackmacgap", "Slack");
        assert_eq!(m.on_focus(Some(&input(slack.clone(), 1)), now).len(), 1);
        assert!(m.on_focus(Some(&input(slack.clone(), 1)), now).is_empty());
        assert_eq!(m.on_focus(Some(&input(slack.clone(), 2)), now).len(), 1);
        let mut secure = input(slack, 3);
        secure.is_secure = true;
        assert!(m.on_focus(Some(&secure), now).is_empty());
        assert!(m.window(now).focus.is_none());
    }

    #[test]
    fn unobservable_apps_leave_no_trace() {
        let mut m = ContextManager::new(OsPlatform::Macos, Duration::from_secs(60));
        let now = Utc::now();
        m.on_app_activated(app("com.apple.notes", "Notes"), Some("Diary".into()), now);
        m.on_unobservable();
        assert!(m.active_app().is_none());
        assert!(m.window_title().is_none());
    }

    #[test]
    fn clear_drops_everything() {
        let mut m = ContextManager::new(OsPlatform::Macos, Duration::from_secs(60));
        let now = Utc::now();
        m.on_app_activated(app("com.apple.notes", "Notes"), Some("Diary".into()), now);
        m.on_clipboard("secret plans for the weekend trip", None, now);
        m.clear();
        assert!(m.clipboard(now).is_none());
        assert!(m.window_title().is_none());
        assert_eq!(m.recent_events().count(), 0);
    }
}
