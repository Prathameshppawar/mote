//! macOS adapter: Accessibility, NSWorkspace, NSPasteboard and Quartz events.

mod ax;
mod keyboard;
mod menu;
mod pasteboard;
mod workspace;

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2_application_services::AXError;

use crate::common::{element_key, input_role, split_at_utf16};
use ax::Element;
use mote_core::platform::*;
use mote_core::text::{head_chars, tail_chars};

/// Re-read text at least this often even if length and selection look unchanged.
const MAX_SNAPSHOT_AGE: Duration = Duration::from_millis(1_500);

/// Attributes of a focused element that do not change while it keeps focus.
#[derive(Clone)]
struct StaticAttributes {
    role: InputRole,
    is_secure: bool,
    is_multiline: bool,
    is_web: bool,
    placeholder: Option<String>,
    label: Option<String>,
}

#[derive(Default)]
struct FocusCache {
    key: u64,
    statics: Option<StaticAttributes>,
    selection: Option<(usize, usize)>,
    length: Option<i64>,
    snapshot: Option<FocusedInput>,
    read_at: Option<Instant>,
}

pub struct MacPlatform {
    system: Element,
    focus: Mutex<FocusCache>,
    apps: Mutex<HashMap<i32, AppInfo>>,
    accessibility_enabled_pids: Mutex<HashSet<i32>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl MacPlatform {
    pub fn new() -> Self {
        Self {
            system: Element::system_wide(),
            focus: Mutex::new(FocusCache::default()),
            apps: Mutex::new(HashMap::new()),
            accessibility_enabled_pids: Mutex::new(HashSet::new()),
        }
    }

    fn app_for_pid(&self, pid: i32) -> Option<AppInfo> {
        if let Some(app) = lock(&self.apps).get(&pid) {
            return Some(app.clone());
        }
        let app = workspace::by_pid(pid)?;
        let mut apps = lock(&self.apps);
        if apps.len() > 256 {
            apps.clear();
        }
        apps.insert(pid, app.clone());
        Some(app)
    }

    /// Chromium-based apps (Electron: Slack, VS Code, Discord, Notion) build
    /// their accessibility tree only when asked; `AXManualAccessibility`
    /// requests it without the side effects of `AXEnhancedUserInterface`.
    fn enable_app_accessibility(&self, pid: i32) {
        let mut done = lock(&self.accessibility_enabled_pids);
        if done.insert(pid) {
            let enabled = Element::application(pid).set_bool("AXManualAccessibility", true);
            tracing::debug!(pid, enabled, "requested accessibility tree");
        }
    }

    fn focused_element(&self) -> Result<Option<Element>, PlatformError> {
        match self.system.element("AXFocusedUIElement") {
            Ok(element) => Ok(Some(element)),
            Err(AXError::APIDisabled) => Err(PlatformError::PermissionDenied),
            Err(_) => Ok(None),
        }
    }

    fn read_statics(element: &Element) -> Option<StaticAttributes> {
        let role = element.string("AXRole").unwrap_or_default();
        let subrole = element.string("AXSubrole");
        let is_secure = role == "AXSecureTextField" || subrole.as_deref() == Some("AXSecureTextField");
        let mapped = input_role(&role, subrole.as_deref());
        let role = match (mapped, is_secure) {
            (Some(role), _) => role,
            (None, true) => InputRole::TextField,
            (None, false) => return None,
        };
        if is_secure {
            return Some(StaticAttributes {
                role,
                is_secure,
                is_multiline: false,
                is_web: false,
                placeholder: None,
                label: None,
            });
        }
        let bounded = |s: Option<String>| s.filter(|s| !s.trim().is_empty()).map(|s| head_chars(&s, 120).to_string());
        Some(StaticAttributes {
            role,
            is_secure,
            is_multiline: role == InputRole::TextArea,
            is_web: element.attribute("AXDOMClassList").is_ok() || element.string("AXDOMIdentifier").is_some(),
            placeholder: bounded(element.string("AXPlaceholderValue")),
            label: bounded(element.string("AXDescription").or_else(|| element.string("AXTitle"))),
        })
    }

    fn read_text(
        element: &Element,
        caret: usize,
        selection_len: usize,
        length: Option<usize>,
        limits: ReadLimits,
    ) -> (String, String, String) {
        if let Some(total) = length {
            let before_start = caret.saturating_sub(limits.before_caret * 2);
            let after_start = caret + selection_len;
            let before = element.string_for_range(before_start, caret - before_start);
            if let Some(before) = before {
                let selected = if selection_len > 0 {
                    element.string_for_range(caret, selection_len.min(limits.selection * 2)).unwrap_or_default()
                } else {
                    String::new()
                };
                let after_len = total.saturating_sub(after_start).min(limits.after_caret * 2);
                let after = if after_len > 0 {
                    element.string_for_range(after_start, after_len).unwrap_or_default()
                } else {
                    String::new()
                };
                return (
                    tail_chars(&before, limits.before_caret).to_string(),
                    head_chars(&selected, limits.selection).to_string(),
                    head_chars(&after, limits.after_caret).to_string(),
                );
            }
        }
        // Fallback: the whole value.
        let value = element.string("AXValue").unwrap_or_default();
        split_at_utf16(&value, caret, selection_len, limits)
    }

    fn caret_rect(element: &Element, caret: usize, total: Option<usize>) -> Option<Rect> {
        let plausible = |r: &objc2_core_foundation::CGRect| {
            r.size.height > 2.0
                && r.size.height < 200.0
                && r.size.width < 2_000.0
                && r.origin.x.is_finite()
                && r.origin.y.is_finite()
        };
        if caret > 0 {
            if let Some(r) = element.bounds_for_range(caret - 1, 1).filter(plausible) {
                return Some(Rect { x: r.origin.x + r.size.width, y: r.origin.y, width: 1.0, height: r.size.height });
            }
        }
        if total.unwrap_or(0) > caret {
            if let Some(r) = element.bounds_for_range(caret, 1).filter(plausible) {
                return Some(Rect { x: r.origin.x, y: r.origin.y, width: 1.0, height: r.size.height });
            }
        }
        if let Some(r) = element.bounds_for_range(caret, 0).filter(plausible) {
            return Some(Rect { x: r.origin.x, y: r.origin.y, width: 1.0, height: r.size.height });
        }
        // Last resort: the bottom-left corner of the field.
        let (position, size) = (element.point("AXPosition")?, element.size("AXSize")?);
        let height = size.height.clamp(14.0, 22.0);
        Some(Rect { x: position.x + 4.0, y: position.y + size.height - height - 2.0, width: 1.0, height })
    }
}

impl Default for MacPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformAdapter for MacPlatform {
    fn os(&self) -> OsPlatform {
        OsPlatform::Macos
    }

    fn coordinate_space(&self) -> CoordinateSpace {
        CoordinateSpace::LogicalPoints
    }

    fn permission_status(&self) -> PermissionStatus {
        PermissionStatus {
            accessibility: if ax::is_trusted() { PermissionState::Granted } else { PermissionState::Denied },
            secure_input_active: ax::secure_input_enabled(),
        }
    }

    fn request_accessibility_permission(&self) -> PermissionState {
        if ax::prompt_for_trust() {
            PermissionState::Granted
        } else {
            PermissionState::Denied
        }
    }

    fn open_permission_settings(&self) -> Result<(), PlatformError> {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status()
            .map_err(|e| PlatformError::Failed(format!("could not open System Settings: {e}")))
            .and_then(|s| {
                if s.success() {
                    Ok(())
                } else {
                    Err(PlatformError::Failed("System Settings did not open".into()))
                }
            })
    }

    fn active_application(&self) -> Option<AppInfo> {
        workspace::frontmost()
    }

    fn active_window(&self) -> Option<WindowInfo> {
        let app = workspace::frontmost()?;
        let pid = i32::try_from(app.pid?).ok()?;
        let window = Element::application(pid).element("AXFocusedWindow").ok()?;
        let title = window.string("AXTitle").map(|t| head_chars(&t, 256).to_string()).filter(|t| !t.is_empty());
        let bounds = match (window.point("AXPosition"), window.size("AXSize")) {
            (Some(p), Some(s)) => Some(Rect { x: p.x, y: p.y, width: s.width, height: s.height }),
            _ => None,
        };
        Some(WindowInfo { title, bounds })
    }

    fn focused_input(&self, limits: ReadLimits) -> Result<Option<FocusedInput>, PlatformError> {
        if !ax::is_trusted() {
            return Err(PlatformError::PermissionDenied);
        }
        let Some(element) = self.focused_element()? else {
            *lock(&self.focus) = FocusCache::default();
            return Ok(None);
        };
        let Some(pid) = element.pid() else { return Ok(None) };
        self.enable_app_accessibility(pid);
        let Some(app) = self.app_for_pid(pid) else { return Ok(None) };
        let key = element_key(&[&pid.to_le_bytes(), &element.hash().to_le_bytes()]);

        let mut cache = lock(&self.focus);
        if cache.key != key {
            *cache = FocusCache { key, statics: Self::read_statics(&element), ..FocusCache::default() };
        }
        let Some(statics) = cache.statics.clone() else { return Ok(None) };
        if statics.is_secure {
            return Ok(Some(FocusedInput {
                app,
                role: statics.role,
                is_secure: true,
                is_multiline: false,
                is_web_content: statics.is_web,
                placeholder: None,
                label: None,
                text_before_caret: String::new(),
                text_after_caret: String::new(),
                selected_text: None,
                total_length: None,
                caret_rect: None,
                element_key: key,
            }));
        }

        let length = element.number("AXNumberOfCharacters").filter(|n| *n >= 0);
        let total = length.and_then(|n| usize::try_from(n).ok());
        let selection = element
            .range("AXSelectedTextRange")
            .and_then(|r| Some((usize::try_from(r.location).ok()?, usize::try_from(r.length).ok()?)));
        let fresh = cache.read_at.is_some_and(|t| t.elapsed() < MAX_SNAPSHOT_AGE);
        if fresh && cache.selection == selection && cache.length == length {
            if let Some(snapshot) = &cache.snapshot {
                return Ok(Some(snapshot.clone()));
            }
        }

        let (caret, selection_len) = selection.unwrap_or((total.unwrap_or(0), 0));
        let (before, selected, after) = Self::read_text(&element, caret, selection_len, total, limits);
        let caret_rect = Self::caret_rect(&element, caret, total);
        let snapshot = FocusedInput {
            app,
            role: statics.role,
            is_secure: false,
            is_multiline: statics.is_multiline,
            is_web_content: statics.is_web,
            placeholder: statics.placeholder.clone(),
            label: statics.label.clone(),
            text_before_caret: before,
            text_after_caret: after,
            selected_text: (!selected.is_empty()).then_some(selected),
            total_length: total,
            caret_rect,
            element_key: key,
        };
        cache.selection = selection;
        cache.length = length;
        cache.snapshot = Some(snapshot.clone());
        cache.read_at = Some(Instant::now());
        Ok(Some(snapshot))
    }

    fn selected_text(&self, max_chars: usize) -> Result<Option<String>, PlatformError> {
        if !ax::is_trusted() {
            return Err(PlatformError::PermissionDenied);
        }
        let Some(element) = self.focused_element()? else { return Ok(None) };
        if Self::read_statics(&element).is_some_and(|s| s.is_secure) {
            return Ok(None);
        }
        Ok(element.string("AXSelectedText").filter(|s| !s.is_empty()).map(|s| head_chars(&s, max_chars).to_string()))
    }

    fn clipboard_sequence(&self) -> u64 {
        pasteboard::change_count()
    }

    fn clipboard_text(&self, max_chars: usize) -> Result<Option<String>, PlatformError> {
        pasteboard::read_text(max_chars)
    }

    fn clipboard_has_non_text(&self) -> bool {
        pasteboard::has_non_text()
    }

    fn set_clipboard_text(&self, text: &str) -> Result<u64, PlatformError> {
        pasteboard::write_text(text)
    }

    fn type_text(&self, text: &str) -> Result<(), PlatformError> {
        if ax::secure_input_enabled() {
            return Err(PlatformError::SecureInput);
        }
        keyboard::type_text(text)
    }

    fn press_key(&self, key: Key, count: usize) -> Result<(), PlatformError> {
        if ax::secure_input_enabled() {
            return Err(PlatformError::SecureInput);
        }
        keyboard::press(key, count)
    }

    fn select_all(&self) -> Result<(), PlatformError> {
        // Prefer setting the selection directly: no keystrokes, no menus.
        let element = self.focused_element()?.ok_or(PlatformError::NoFocusedElement)?;
        if let Some(length) = element.number("AXNumberOfCharacters").and_then(|n| usize::try_from(n).ok()) {
            if element.set_selected_range(0, length) {
                return Ok(());
            }
        }
        let pid = element.pid().ok_or(PlatformError::NoFocusedElement)?;
        menu::press_shortcut(pid, 'a')
    }

    fn paste(&self) -> Result<(), PlatformError> {
        if ax::secure_input_enabled() {
            return Err(PlatformError::SecureInput);
        }
        let element = self.focused_element()?.ok_or(PlatformError::NoFocusedElement)?;
        let pid = element.pid().ok_or(PlatformError::NoFocusedElement)?;
        menu::press_shortcut(pid, 'v')
    }

    fn activate_application(&self, app: &AppInfo) -> Result<(), PlatformError> {
        workspace::activate(app)
    }

    fn running_applications(&self) -> Vec<AppInfo> {
        workspace::running()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_platform_basics_without_panicking() {
        let p = MacPlatform::new();
        assert_eq!(p.os(), OsPlatform::Macos);
        assert_eq!(p.coordinate_space(), CoordinateSpace::LogicalPoints);
        let status = p.permission_status();
        assert!(matches!(status.accessibility, PermissionState::Granted | PermissionState::Denied));
        // Without permission the adapter must refuse cleanly; with it, reading must not fail.
        match p.focused_input(ReadLimits::default()) {
            Err(PlatformError::PermissionDenied) => assert_eq!(status.accessibility, PermissionState::Denied),
            Ok(_) => {}
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn frontmost_and_running_applications() {
        let p = MacPlatform::new();
        // CI runners and developer machines always have some GUI application.
        let running = p.running_applications();
        assert!(running.iter().all(|a| !a.id.is_empty() && !a.name.is_empty()));
        if let Some(front) = p.active_application() {
            assert!(!front.id.is_empty());
        }
    }

    #[test]
    fn clipboard_sequence_is_readable() {
        let p = MacPlatform::new();
        let a = p.clipboard_sequence();
        let b = p.clipboard_sequence();
        assert_eq!(a, b, "reading the sequence does not change it");
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// Prints what the adapter sees (metadata only). Run manually:
    /// `cargo test -p mote-platform probe -- --ignored --nocapture`
    #[test]
    #[ignore = "manual probe"]
    fn probe_focused_input_metadata() {
        let p = MacPlatform::new();
        eprintln!("permission: {:?}", p.permission_status());
        eprintln!("frontmost: {:?}", p.active_application().map(|a| (a.id, a.name)));
        eprintln!("window title present: {:?}", p.active_window().map(|w| w.title.is_some()));
        match p.focused_input(ReadLimits::default()) {
            Ok(Some(f)) => eprintln!(
                "focused: app={} role={:?} secure={} web={} before_len={} after_len={} caret={:?} placeholder_present={}",
                f.app.id,
                f.role,
                f.is_secure,
                f.is_web_content,
                f.text_before_caret.chars().count(),
                f.text_after_caret.chars().count(),
                f.caret_rect,
                f.placeholder.is_some()
            ),
            other => eprintln!("focused: {:?}", other.map(|o| o.map(|f| f.role))),
        }
        eprintln!("running apps: {}", p.running_applications().len());
    }
}
