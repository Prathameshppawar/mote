//! Test doubles for the platform and the UI shell.

use std::sync::Mutex;

use crate::assistance::ClipboardWriteLog;
use crate::engine::{AssistantShell, EngineStatus, OverlayView};
use crate::platform::*;
use crate::text::grapheme_count;

/// What a fake platform was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformAction {
    Typed(String),
    Key(Key, usize),
    SelectAll,
    Paste,
}

/// An in-memory platform. Typing and Backspace edit the focused input so
/// follow-up reads see the result, as in a real application.
#[derive(Default)]
pub struct FakePlatform {
    pub focused: Mutex<Option<FocusedInput>>,
    pub app: Mutex<Option<AppInfo>>,
    pub actions: Mutex<Vec<PlatformAction>>,
    pub clipboard: Mutex<(u64, Option<String>)>,
    pub permission: Mutex<Option<PermissionStatus>>,
    /// Makes `paste()` fail as if the app had no paste command.
    pub paste_fails: std::sync::atomic::AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl FakePlatform {
    pub fn set_focus(&self, input: Option<FocusedInput>) {
        if let Some(i) = &input {
            *lock(&self.app) = Some(i.app.clone());
        }
        *lock(&self.focused) = input;
    }

    pub fn actions(&self) -> Vec<PlatformAction> {
        lock(&self.actions).clone()
    }

    pub fn typed(&self) -> Vec<String> {
        self.actions()
            .into_iter()
            .filter_map(|a| match a {
                PlatformAction::Typed(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    pub fn text_before_caret(&self) -> Option<String> {
        lock(&self.focused).as_ref().map(|f| f.text_before_caret.clone())
    }
}

impl PlatformAdapter for FakePlatform {
    fn os(&self) -> OsPlatform {
        OsPlatform::Macos
    }

    fn coordinate_space(&self) -> CoordinateSpace {
        CoordinateSpace::LogicalPoints
    }

    fn permission_status(&self) -> PermissionStatus {
        lock(&self.permission)
            .unwrap_or(PermissionStatus { accessibility: PermissionState::Granted, secure_input_active: false })
    }

    fn request_accessibility_permission(&self) -> PermissionState {
        self.permission_status().accessibility
    }

    fn open_permission_settings(&self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn active_application(&self) -> Option<AppInfo> {
        lock(&self.app).clone()
    }

    fn active_window(&self) -> Option<WindowInfo> {
        Some(WindowInfo::default())
    }

    fn focused_input(&self, _limits: ReadLimits) -> Result<Option<FocusedInput>, PlatformError> {
        Ok(lock(&self.focused).clone())
    }

    fn selected_text(&self, _max_chars: usize) -> Result<Option<String>, PlatformError> {
        Ok(lock(&self.focused).as_ref().and_then(|f| f.selected_text.clone()))
    }

    fn clipboard_sequence(&self) -> u64 {
        lock(&self.clipboard).0
    }

    fn clipboard_text(&self, _max_chars: usize) -> Result<Option<String>, PlatformError> {
        Ok(lock(&self.clipboard).1.clone())
    }

    fn clipboard_has_non_text(&self) -> bool {
        false
    }

    fn set_clipboard_text(&self, text: &str) -> Result<u64, PlatformError> {
        let mut clip = lock(&self.clipboard);
        clip.0 += 1;
        clip.1 = Some(text.to_string());
        Ok(clip.0)
    }

    fn type_text(&self, text: &str) -> Result<(), PlatformError> {
        lock(&self.actions).push(PlatformAction::Typed(text.to_string()));
        if let Some(f) = lock(&self.focused).as_mut() {
            f.text_before_caret.push_str(text);
        }
        Ok(())
    }

    fn press_key(&self, key: Key, count: usize) -> Result<(), PlatformError> {
        lock(&self.actions).push(PlatformAction::Key(key, count));
        if key == Key::Backspace {
            if let Some(f) = lock(&self.focused).as_mut() {
                let keep = grapheme_count(&f.text_before_caret).saturating_sub(count);
                f.text_before_caret =
                    unicode_segmentation::UnicodeSegmentation::graphemes(f.text_before_caret.as_str(), true)
                        .take(keep)
                        .collect();
            }
        }
        Ok(())
    }

    fn select_all(&self) -> Result<(), PlatformError> {
        lock(&self.actions).push(PlatformAction::SelectAll);
        Ok(())
    }

    fn paste(&self) -> Result<(), PlatformError> {
        if self.paste_fails.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(PlatformError::NotSupported("no paste command".into()));
        }
        lock(&self.actions).push(PlatformAction::Paste);
        let text = lock(&self.clipboard).1.clone().unwrap_or_default();
        if let Some(f) = lock(&self.focused).as_mut() {
            f.text_before_caret.push_str(&text);
        }
        Ok(())
    }

    fn activate_application(&self, _app: &AppInfo) -> Result<(), PlatformError> {
        Ok(())
    }

    fn running_applications(&self) -> Vec<AppInfo> {
        lock(&self.app).clone().into_iter().collect()
    }
}

/// What a fake shell was asked to do.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellCall {
    Show(OverlayView),
    Hide,
    Keys(bool),
    Status(EngineStatus),
}

/// Records every call from the engine.
#[derive(Default)]
pub struct FakeShell {
    pub calls: Mutex<Vec<ShellCall>>,
}

impl FakeShell {
    pub fn calls(&self) -> Vec<ShellCall> {
        lock(&self.calls).clone()
    }

    /// The view currently on screen, if any.
    pub fn visible(&self) -> Option<OverlayView> {
        let mut visible = None;
        for call in self.calls() {
            match call {
                ShellCall::Show(v) => visible = Some(v),
                ShellCall::Hide => visible = None,
                _ => {}
            }
        }
        visible
    }

    /// Whether suggestion keys are currently registered.
    pub fn keys_active(&self) -> bool {
        self.calls()
            .into_iter()
            .rev()
            .find_map(|c| match c {
                ShellCall::Keys(active) => Some(active),
                _ => None,
            })
            .unwrap_or(false)
    }

    pub fn last_status(&self) -> Option<EngineStatus> {
        self.calls().into_iter().rev().find_map(|c| match c {
            ShellCall::Status(s) => Some(s),
            _ => None,
        })
    }
}

impl AssistantShell for FakeShell {
    fn show(&self, view: &OverlayView) {
        lock(&self.calls).push(ShellCall::Show(view.clone()));
    }

    fn hide(&self) {
        lock(&self.calls).push(ShellCall::Hide);
    }

    fn set_suggestion_keys(&self, active: bool) {
        lock(&self.calls).push(ShellCall::Keys(active));
    }

    fn status(&self, status: &EngineStatus) {
        lock(&self.calls).push(ShellCall::Status(status.clone()));
    }
}

/// Remembers clipboard sequences written by Mote.
#[derive(Default)]
pub struct RecordingClipboardLog {
    pub sequences: Mutex<Vec<u64>>,
}

impl ClipboardWriteLog for RecordingClipboardLog {
    fn record_own_write(&self, sequence: u64) {
        lock(&self.sequences).push(sequence);
    }
}

/// A focused input with sensible defaults for tests.
pub fn focused_input(app: AppInfo, element_key: u64, text_before: &str) -> FocusedInput {
    FocusedInput {
        app,
        role: InputRole::TextArea,
        is_secure: false,
        is_multiline: true,
        is_web_content: false,
        placeholder: None,
        label: None,
        text_before_caret: text_before.to_string(),
        text_after_caret: String::new(),
        selected_text: None,
        total_length: None,
        caret_rect: Some(Rect { x: 100.0, y: 200.0, width: 1.0, height: 18.0 }),
        element_key,
    }
}
