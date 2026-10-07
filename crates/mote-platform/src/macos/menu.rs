//! Performs standard editing commands through the application's menu bar.
//!
//! Instead of synthesizing ⌘V or ⌘A (whose key positions depend on the
//! keyboard layout), Mote presses the menu item whose key equivalent is ⌘V or
//! ⌘A. Key equivalents are locale-independent, and pressing the menu item runs
//! the app's real paste/select-all code path, which web and Electron apps
//! handle like a user action.

use objc2_application_services::AXError;
use objc2_core_foundation::CFString;

use super::ax::Element;
use mote_core::platform::PlatformError;

/// `AXMenuItemCmdModifiers` value for "⌘ only".
const COMMAND_ONLY: i64 = 0;

fn matches_shortcut(item: &Element, letter: char) -> bool {
    let Some(cmd_char) = item.string("AXMenuItemCmdChar") else { return false };
    let modifiers = item.number("AXMenuItemCmdModifiers").unwrap_or(COMMAND_ONLY);
    cmd_char.eq_ignore_ascii_case(&letter.to_string()) && modifiers == COMMAND_ONLY
}

/// Finds the first-level menu item with key equivalent ⌘`letter`.
fn find_item(app: &Element, letter: char) -> Option<Element> {
    let menu_bar = app.element("AXMenuBar").ok()?;
    let mut top_level = menu_bar.children();
    // The Edit menu is conventionally third (Apple, App, Edit); look there first.
    if top_level.len() > 2 {
        let edit = top_level.remove(2);
        top_level.insert(0, edit);
    }
    for top in top_level {
        for menu in top.children() {
            for item in menu.children() {
                if matches_shortcut(&item, letter) {
                    return Some(item);
                }
            }
        }
    }
    None
}

/// Presses the ⌘`letter` menu item of the application with `pid`.
pub fn press_shortcut(pid: i32, letter: char) -> Result<(), PlatformError> {
    let app = Element::application(pid);
    let item = find_item(&app, letter).ok_or_else(|| {
        PlatformError::NotSupported(format!("this application has no ⌘{} menu command", letter.to_ascii_uppercase()))
    })?;
    if item.bool("AXEnabled") == Some(false) {
        return Err(PlatformError::NotSupported("the menu command is disabled".into()));
    }
    let action = CFString::from_str("AXPress");
    // SAFETY: AXPress is a standard action name; the element is a valid menu item.
    let error = unsafe { item.0.perform_action(&action) };
    if error == AXError::Success {
        Ok(())
    } else {
        Err(PlatformError::Failed(format!("menu command failed (AX error {})", error.0)))
    }
}
