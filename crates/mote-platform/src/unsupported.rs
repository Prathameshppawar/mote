//! Adapter for operating systems Mote does not support (builds and tests only).

use mote_core::platform::*;

pub struct UnsupportedPlatform;

fn unsupported<T>() -> Result<T, PlatformError> {
    Err(PlatformError::NotSupported("this operating system is not supported".into()))
}

impl PlatformAdapter for UnsupportedPlatform {
    fn os(&self) -> OsPlatform {
        OsPlatform::Other
    }
    fn coordinate_space(&self) -> CoordinateSpace {
        CoordinateSpace::PhysicalPixels
    }
    fn permission_status(&self) -> PermissionStatus {
        PermissionStatus { accessibility: PermissionState::Unknown, secure_input_active: false }
    }
    fn request_accessibility_permission(&self) -> PermissionState {
        PermissionState::Unknown
    }
    fn open_permission_settings(&self) -> Result<(), PlatformError> {
        unsupported()
    }
    fn active_application(&self) -> Option<AppInfo> {
        None
    }
    fn active_window(&self) -> Option<WindowInfo> {
        None
    }
    fn focused_input(&self, _limits: ReadLimits) -> Result<Option<FocusedInput>, PlatformError> {
        unsupported()
    }
    fn selected_text(&self, _max_chars: usize) -> Result<Option<String>, PlatformError> {
        unsupported()
    }
    fn clipboard_sequence(&self) -> u64 {
        0
    }
    fn clipboard_text(&self, _max_chars: usize) -> Result<Option<String>, PlatformError> {
        unsupported()
    }
    fn clipboard_has_non_text(&self) -> bool {
        false
    }
    fn set_clipboard_text(&self, _text: &str) -> Result<u64, PlatformError> {
        unsupported()
    }
    fn type_text(&self, _text: &str) -> Result<(), PlatformError> {
        unsupported()
    }
    fn press_key(&self, _key: Key, _count: usize) -> Result<(), PlatformError> {
        unsupported()
    }
    fn select_all(&self) -> Result<(), PlatformError> {
        unsupported()
    }
    fn paste(&self) -> Result<(), PlatformError> {
        unsupported()
    }
    fn activate_application(&self, _app: &AppInfo) -> Result<(), PlatformError> {
        unsupported()
    }
    fn running_applications(&self) -> Vec<AppInfo> {
        Vec::new()
    }
}
