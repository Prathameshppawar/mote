//! The platform abstraction.
//!
//! The core never calls OS APIs directly. Everything Mote observes or does on
//! the desktop goes through [`PlatformAdapter`], implemented per OS in the
//! `mote-platform` crate (macOS Accessibility, Windows UI Automation) and by
//! fakes in tests. Overlay windows and global shortcuts are UI concerns and live
//! behind [`crate::engine::AssistantShell`] instead.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The operating system Mote runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OsPlatform {
    Macos,
    Windows,
    Other,
}

impl OsPlatform {
    /// Platform of the running process.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Other
        }
    }

    /// Stable identifier used as the `source` of context events.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Other => "other",
        }
    }
}

/// An application as seen by the OS.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Stable identifier: the bundle identifier on macOS (`com.tinyspeck.slackmacgap`),
    /// the lowercase executable name on Windows (`slack.exe`).
    pub id: String,
    /// Display name (`Slack`).
    pub name: String,
    #[serde(default)]
    pub pid: Option<u32>,
}

impl AppInfo {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self { id: id.into(), name: name.into(), pid: None }
    }
}

/// A rectangle in screen coordinates.
///
/// On macOS these are logical points with the origin at the top-left of the
/// primary display; on Windows they are physical pixels. See [`CoordinateSpace`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn is_plausible(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.width >= 0.0 && self.height > 0.0 && self.height < 400.0
    }
}

/// Which coordinate system a [`Rect`] uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateSpace {
    LogicalPoints,
    PhysicalPixels,
}

/// The active window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowInfo {
    /// Window title. Kept in memory only; never persisted or logged.
    pub title: Option<String>,
    pub bounds: Option<Rect>,
}

/// The kind of text control that has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum InputRole {
    /// Multi-line plain text (AXTextArea, Win32 multi-line edit).
    TextArea,
    /// Single-line text field.
    TextField,
    /// Search box (AXSearchField subrole, UIA search edit).
    SearchField,
    /// Combo box with an editable field (browser address bar, pickers).
    ComboBox,
    /// Rich document surface (contenteditable, UIA Document).
    Document,
    /// Terminal emulator surface.
    Terminal,
    Unknown,
}

/// A read-only snapshot of the focused text input.
///
/// Text is bounded by [`ReadLimits`] and lives in memory only. It is never
/// logged, persisted or sent anywhere unless a feature explicitly needs it and
/// the privacy policy allows it.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusedInput {
    pub app: AppInfo,
    pub role: InputRole,
    /// Password and other secure fields. Mote never reads or assists these.
    pub is_secure: bool,
    pub is_multiline: bool,
    /// Whether the field belongs to web content (browser or Electron app).
    pub is_web_content: bool,
    /// Placeholder text, e.g. "Message #general" or "Ask anything".
    pub placeholder: Option<String>,
    /// Accessible label/description of the field.
    pub label: Option<String>,
    /// Text before the caret, at most `ReadLimits::before_caret` characters.
    pub text_before_caret: String,
    /// Text after the caret, at most `ReadLimits::after_caret` characters.
    pub text_after_caret: String,
    /// Selected text, at most `ReadLimits::selection` characters.
    pub selected_text: Option<String>,
    /// Total length of the field in UTF-16 code units, when known.
    pub total_length: Option<usize>,
    /// Screen rectangle of the caret, when the app exposes it.
    pub caret_rect: Option<Rect>,
    /// Identifies the focused element; changes when focus moves to another field.
    pub element_key: u64,
}

impl FocusedInput {
    /// Whether the caret sits at the end of the field's content (ignoring
    /// trailing whitespace), which is where inline completion is offered.
    pub fn caret_at_end(&self) -> bool {
        self.text_after_caret.trim().is_empty() || self.text_after_caret.starts_with('\n')
    }
}

/// How much text the platform layer reads around the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimits {
    pub before_caret: usize,
    pub after_caret: usize,
    pub selection: usize,
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self { before_caret: 2_000, after_caret: 200, selection: 8_000 }
    }
}

/// State of an OS permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Granted,
    Denied,
    /// The platform has no such permission (Windows UI Automation).
    NotRequired,
    Unknown,
}

/// Permissions and input-security state relevant to Mote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub accessibility: PermissionState,
    /// macOS Secure Event Input is active (a password field or a secure
    /// terminal has focus somewhere); Mote must not observe or type.
    pub secure_input_active: bool,
}

impl PermissionStatus {
    pub fn can_observe(&self) -> bool {
        matches!(self.accessibility, PermissionState::Granted | PermissionState::NotRequired)
            && !self.secure_input_active
    }
}

/// Keys Mote can synthesize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Backspace,
    Tab,
    Escape,
    /// Shift+Enter: inserts a line break without submitting in chat apps.
    ShiftEnter,
    Left,
    Right,
}

/// Errors from platform operations.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PlatformError {
    #[error("accessibility permission has not been granted")]
    PermissionDenied,
    #[error("secure input is active; Mote cannot read or type")]
    SecureInput,
    #[error("no focused element")]
    NoFocusedElement,
    #[error("not supported on this platform or application: {0}")]
    NotSupported(String),
    #[error("platform call failed: {0}")]
    Failed(String),
}

/// Everything Mote needs from the operating system.
///
/// Implementations must be cheap to call repeatedly (the observer polls them)
/// and must never block for long: every call should time out quickly when the
/// target application is unresponsive.
pub trait PlatformAdapter: Send + Sync {
    fn os(&self) -> OsPlatform;

    /// Coordinate space of rectangles returned by this adapter.
    fn coordinate_space(&self) -> CoordinateSpace;

    /// Current permission state. Must not prompt the user.
    fn permission_status(&self) -> PermissionStatus;

    /// Asks the OS to show its permission prompt, where one exists.
    fn request_accessibility_permission(&self) -> PermissionState;

    /// Opens the OS settings page where the user grants permissions.
    fn open_permission_settings(&self) -> Result<(), PlatformError>;

    /// getActiveApplication()
    fn active_application(&self) -> Option<AppInfo>;

    /// getActiveWindow()
    fn active_window(&self) -> Option<WindowInfo>;

    /// detectFocusedInput(): the focused text input, if any. Secure fields are
    /// reported with `is_secure = true` and empty text.
    fn focused_input(&self, limits: ReadLimits) -> Result<Option<FocusedInput>, PlatformError>;

    /// getSelectedText()
    fn selected_text(&self, max_chars: usize) -> Result<Option<String>, PlatformError>;

    /// A counter that changes whenever the clipboard changes. Cheap; never reads content.
    fn clipboard_sequence(&self) -> u64;

    /// getClipboard(): current clipboard text, bounded.
    fn clipboard_text(&self, max_chars: usize) -> Result<Option<String>, PlatformError>;

    /// Whether the clipboard holds data other than plain text (images, files,
    /// rich content) that a text round-trip would lose.
    fn clipboard_has_non_text(&self) -> bool;

    /// Replaces the clipboard with plain text the user keeps (an explicit copy,
    /// or their own content put back after a paste); returns the new sequence number.
    fn set_clipboard_text(&self, text: &str) -> Result<u64, PlatformError>;

    /// Replaces the clipboard with text that exists only to be pasted by Mote.
    /// It is marked so that clipboard history and monitors, including
    /// [`Self::clipboard_text`], ignore it. Returns the new sequence number.
    fn set_transient_clipboard_text(&self, text: &str) -> Result<u64, PlatformError>;

    /// insertText(): types `text` at the caret of the focused application.
    /// `text` must not contain line breaks (they would submit chat messages);
    /// use [`Key::ShiftEnter`] or paste for multi-line text.
    fn type_text(&self, text: &str) -> Result<(), PlatformError>;

    /// Presses `key` `count` times.
    fn press_key(&self, key: Key, count: usize) -> Result<(), PlatformError>;

    /// Selects all text in the focused field (Cmd/Ctrl+A).
    fn select_all(&self) -> Result<(), PlatformError>;

    /// Pastes the clipboard into the focused field (Cmd/Ctrl+V).
    fn paste(&self) -> Result<(), PlatformError>;

    /// Brings `app` back to the foreground (after Mote's palette took focus).
    fn activate_application(&self, app: &AppInfo) -> Result<(), PlatformError>;

    /// Applications with a user interface, for the exclusion picker.
    fn running_applications(&self) -> Vec<AppInfo>;
}
