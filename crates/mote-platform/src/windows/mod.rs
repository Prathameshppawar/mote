//! Windows adapter.
//!
//! * UI Automation (UIA) for the focused input: role, text around the caret,
//!   selection and caret bounds ([`uia`]).
//! * Win32 for the foreground window, processes and application names
//!   ([`process`]).
//! * The clipboard API for plain text ([`clipboard`]).
//! * `SendInput` for typing and shortcuts ([`input`]).
//!
//! Privacy: nothing in this module logs text, clipboard content, window titles
//! or key data. Debug logs carry metadata only (HRESULT codes, counts), and
//! every string read from another application is bounded.

mod clipboard;
mod com;
mod input;
mod process;
mod uia;

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use mote_core::platform::{
    AppInfo, CoordinateSpace, FocusedInput, Key, OsPlatform, PermissionState, PermissionStatus, PlatformAdapter,
    PlatformError, ReadLimits, WindowInfo,
};
use mote_core::text::head_chars;

/// Window titles are kept in memory only; this bounds how much is read.
const TITLE_MAX_CHARS: usize = 512;
/// After a UI Automation call to an application times out, it is left alone
/// for this long so a hung application cannot stall every observer tick.
const HUNG_BACKOFF: Duration = Duration::from_secs(2);

/// The Windows implementation of [`PlatformAdapter`].
///
/// All methods are safe to call from any thread. UI Automation calls join the
/// calling thread to COM's multithreaded apartment on first use.
pub struct WindowsPlatform {
    automation: com::SharedAutomation,
    apps: process::AppCache,
    /// Process that recently timed out a UI Automation call, and until when it is skipped.
    backoff: Mutex<Option<(u32, Instant)>>,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            automation: com::SharedAutomation::default(),
            apps: process::AppCache::default(),
            backoff: Mutex::new(None),
        }
    }

    /// The foreground application whose focused element may be inspected, or
    /// `None` when there is nothing to read: no foreground window, Mote's own
    /// window, an application that is not responding, or one that recently
    /// timed out.
    fn ui_target(&self) -> Option<uia::Target> {
        let foreground = process::foreground()?;
        if foreground.pid == std::process::id() || foreground.is_hung() || self.backing_off(foreground.pid) {
            return None;
        }
        let app = self.apps.app_for_pid(foreground.pid)?;
        Some(uia::Target {
            app,
            pid: foreground.pid,
            window_class: foreground.class_name,
            thread_id: foreground.thread_id,
        })
    }

    fn backing_off(&self, pid: u32) -> bool {
        let backoff = self.backoff.lock().unwrap_or_else(PoisonError::into_inner);
        matches!(*backoff, Some((hung, until)) if hung == pid && Instant::now() < until)
    }

    /// Turns a UI Automation failure into "nothing to read" (timeouts start a
    /// back-off for the application), and discards a result when another
    /// application took the foreground during the read, so text is never
    /// attributed to the wrong application.
    fn settle<T>(&self, pid: u32, result: ::windows::core::Result<Option<T>>) -> Option<T> {
        match result {
            Ok(value) => value.filter(|_| process::foreground().is_some_and(|foreground| foreground.pid == pid)),
            Err(error) => {
                if uia::is_timeout(&error) {
                    *self.backoff.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some((pid, Instant::now() + HUNG_BACKOFF));
                }
                tracing::debug!(hresult = %hresult(&error), "UI Automation read failed");
                None
            }
        }
    }
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformAdapter for WindowsPlatform {
    fn os(&self) -> OsPlatform {
        OsPlatform::Windows
    }

    fn coordinate_space(&self) -> CoordinateSpace {
        // The app is per-monitor DPI aware, so UIA and Win32 report physical pixels.
        CoordinateSpace::PhysicalPixels
    }

    fn permission_status(&self) -> PermissionStatus {
        PermissionStatus { accessibility: PermissionState::NotRequired, secure_input_active: false }
    }

    fn request_accessibility_permission(&self) -> PermissionState {
        PermissionState::NotRequired
    }

    fn open_permission_settings(&self) -> Result<(), PlatformError> {
        // UI Automation needs no permission on Windows: there is nothing to open.
        Ok(())
    }

    fn active_application(&self) -> Option<AppInfo> {
        let foreground = process::foreground()?;
        self.apps.app_for_pid(foreground.pid)
    }

    fn active_window(&self) -> Option<WindowInfo> {
        let foreground = process::foreground()?;
        Some(WindowInfo {
            title: process::window_title(foreground.hwnd, TITLE_MAX_CHARS),
            bounds: process::window_rect(foreground.hwnd),
        })
    }

    fn focused_input(&self, limits: ReadLimits) -> Result<Option<FocusedInput>, PlatformError> {
        let Some(target) = self.ui_target() else {
            return Ok(None);
        };
        let automation = self.automation.get()?;
        Ok(self.settle(target.pid, uia::focused_input(&automation, &target, limits)))
    }

    fn selected_text(&self, max_chars: usize) -> Result<Option<String>, PlatformError> {
        if max_chars == 0 {
            return Ok(None);
        }
        let Some(target) = self.ui_target() else {
            return Ok(None);
        };
        let automation = self.automation.get()?;
        Ok(self.settle(target.pid, uia::selected_text(&automation, max_chars)))
    }

    fn clipboard_sequence(&self) -> u64 {
        clipboard::sequence()
    }

    fn clipboard_text(&self, max_chars: usize) -> Result<Option<String>, PlatformError> {
        clipboard::text(max_chars)
    }

    fn clipboard_has_non_text(&self) -> bool {
        clipboard::has_non_text()
    }

    fn set_clipboard_text(&self, text: &str) -> Result<u64, PlatformError> {
        clipboard::set_text(text)
    }

    fn type_text(&self, text: &str) -> Result<(), PlatformError> {
        input::type_text(text)
    }

    fn press_key(&self, key: Key, count: usize) -> Result<(), PlatformError> {
        input::press_key(key, count)
    }

    fn select_all(&self) -> Result<(), PlatformError> {
        input::select_all()
    }

    fn paste(&self) -> Result<(), PlatformError> {
        input::paste()
    }

    fn activate_application(&self, app: &AppInfo) -> Result<(), PlatformError> {
        let by_pid = app.pid.and_then(|pid| process::app_window(|candidate| candidate == pid));
        // The process may have been restarted since `app` was captured: fall back to its executable.
        let window = by_pid.or_else(|| {
            process::app_window(|candidate| self.apps.app_for_pid(candidate).is_some_and(|found| found.id == app.id))
        });
        let Some((hwnd, pid)) = window else {
            return Err(PlatformError::Failed("the application has no window to activate".into()));
        };
        if process::bring_to_front(hwnd, pid) {
            Ok(())
        } else {
            Err(PlatformError::Failed("Windows did not allow bringing the application to the foreground".into()))
        }
    }

    fn running_applications(&self) -> Vec<AppInfo> {
        let mut seen = HashSet::new();
        let mut apps: Vec<AppInfo> = Vec::new();
        for (_, pid) in process::app_windows() {
            if !seen.insert(pid) {
                continue;
            }
            let Some(app) = self.apps.app_for_pid(pid) else { continue };
            if process::is_shell_host(&app.id) || apps.iter().any(|known| known.id == app.id) {
                continue;
            }
            apps.push(app);
        }
        apps.sort_by_cached_key(|app| (app.name.to_lowercase(), app.id.clone()));
        apps
    }
}

/// Formats an HRESULT for debug logs (metadata only).
fn hresult(error: &::windows::core::Error) -> String {
    format!("{:#010x}", error.code().0)
}

fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..=0xDBFF).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

/// The first `max_units` UTF-16 units of `units`, without splitting a surrogate pair.
fn head_units(units: &[u16], max_units: usize) -> &[u16] {
    let mut end = units.len().min(max_units);
    if end > 0 && end < units.len() && is_high_surrogate(units[end - 1]) {
        end -= 1;
    }
    &units[..end]
}

/// The last `max_units` UTF-16 units of `units`, without splitting a surrogate pair.
fn tail_units(units: &[u16], max_units: usize) -> &[u16] {
    let mut start = units.len().saturating_sub(max_units);
    if start > 0 && start < units.len() && is_low_surrogate(units[start]) {
        start += 1;
    }
    &units[start..]
}

/// Converts at most `max_chars` characters from the start of `units`.
fn bounded_utf16(units: &[u16], max_chars: usize) -> String {
    let text = String::from_utf16_lossy(head_units(units, max_chars.saturating_mul(2)));
    head_chars(&text, max_chars).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_windows_capabilities_without_permissions() {
        let platform = WindowsPlatform::new();
        assert_eq!(platform.os(), OsPlatform::Windows);
        assert_eq!(platform.coordinate_space(), CoordinateSpace::PhysicalPixels);
        let status = platform.permission_status();
        assert_eq!(status.accessibility, PermissionState::NotRequired);
        assert!(!status.secure_input_active);
        assert!(status.can_observe());
        assert_eq!(platform.request_accessibility_permission(), PermissionState::NotRequired);
        assert_eq!(platform.open_permission_settings(), Ok(()));
    }

    #[test]
    fn utf16_bounds_never_split_surrogate_pairs() {
        let units: Vec<u16> = "a😀b".encode_utf16().collect(); // a, high, low, b
        assert_eq!(head_units(&units, 2), &units[..1], "a cut inside the pair drops the high surrogate");
        assert_eq!(head_units(&units, 3), &units[..3]);
        assert_eq!(head_units(&units, 99), &units[..]);
        assert_eq!(tail_units(&units, 2), &units[3..], "a cut inside the pair drops the low surrogate");
        assert_eq!(tail_units(&units, 3), &units[1..]);
        assert_eq!(tail_units(&units, 99), &units[..]);
        assert_eq!(bounded_utf16(&units, 2), "a😀");
        assert_eq!(bounded_utf16(&units, 1), "a");
        assert_eq!(bounded_utf16(&units, 0), "");
    }

    #[test]
    fn hresults_are_formatted_as_hex() {
        let error = ::windows::core::Error::from_hresult(::windows::Win32::Foundation::E_ACCESSDENIED);
        assert_eq!(hresult(&error), "0x80070005");
    }

    #[test]
    fn backoff_applies_only_to_the_hung_process() {
        let platform = WindowsPlatform::new();
        *platform.backoff.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((42, Instant::now() + Duration::from_secs(60)));
        assert!(platform.backing_off(42));
        assert!(!platform.backing_off(43));
        *platform.backoff.lock().unwrap_or_else(PoisonError::into_inner) = Some((42, Instant::now()));
        assert!(!platform.backing_off(42), "expired");
    }

    /// Runs against whatever has focus on the machine (often nothing on CI):
    /// the reads must succeed or report "no input", never fail or panic.
    #[test]
    fn reading_the_focused_input_tolerates_any_desktop_state() {
        let platform = WindowsPlatform::new();
        let input = platform.focused_input(ReadLimits { before_caret: 50, after_caret: 10, selection: 20 });
        let input = input.expect("UI Automation is available");
        if let Some(input) = input {
            assert!(input.text_before_caret.chars().count() <= 50);
            assert!(input.text_after_caret.chars().count() <= 10);
            assert!(input.selected_text.is_none_or(|s| s.chars().count() <= 20));
            if input.is_secure {
                assert!(input.text_before_caret.is_empty() && input.text_after_caret.is_empty());
            }
        }
        let selection = platform.selected_text(20).expect("UI Automation is available");
        assert!(selection.is_none_or(|s| s.chars().count() <= 20));
        // Works from another thread too (the adapter is shared across threads).
        let handle = std::thread::spawn(move || platform.focused_input(ReadLimits::default()).is_ok());
        assert!(handle.join().unwrap_or(false));
    }

    #[test]
    fn foreground_queries_do_not_fail() {
        let platform = WindowsPlatform::new();
        if let Some(app) = platform.active_application() {
            assert!(!app.id.is_empty() && app.id == app.id.to_lowercase());
            assert!(!app.name.is_empty());
            assert!(app.pid.is_some());
        }
        if let Some(window) = platform.active_window() {
            assert!(window.title.is_none_or(|title| title.chars().count() <= TITLE_MAX_CHARS));
        }
    }

    #[test]
    fn running_applications_are_unique_and_sorted() {
        let apps = WindowsPlatform::new().running_applications();
        let ids: HashSet<&str> = apps.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(ids.len(), apps.len(), "deduplicated by id");
        let names: Vec<String> = apps.iter().map(|app| app.name.to_lowercase()).collect();
        assert!(names.windows(2).all(|pair| pair[0] <= pair[1]), "sorted by name");
    }

    #[test]
    fn activating_an_application_without_windows_fails_cleanly() {
        let platform = WindowsPlatform::new();
        let ghost = AppInfo { id: "mote-test-no-such-app.exe".into(), name: "Nothing".into(), pid: Some(u32::MAX - 1) };
        assert!(matches!(platform.activate_application(&ghost), Err(PlatformError::Failed(_))));
    }
}
