//! # mote-platform
//!
//! OS integrations behind [`mote_core::platform::PlatformAdapter`]:
//!
//! * **macOS**: Accessibility (AXUIElement) for focused-input text, selection
//!   and caret bounds; NSWorkspace for applications; NSPasteboard for the
//!   clipboard; Quartz events for typing.
//! * **Windows**: UI Automation for focused-input text and caret bounds;
//!   Win32 for the foreground window and process; the clipboard API; and
//!   `SendInput` for typing.
//! * Other platforms get an adapter that reports everything as unsupported, so
//!   the workspace still builds and the core can be tested there.

pub mod common;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;

use mote_core::platform::PlatformAdapter;

/// Creates the adapter for the running operating system.
pub fn create() -> Arc<dyn PlatformAdapter> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacPlatform::new())
    }
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsPlatform::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Arc::new(unsupported::UnsupportedPlatform)
    }
}
