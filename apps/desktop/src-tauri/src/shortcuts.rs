//! Global shortcuts.
//!
//! The command-palette shortcut is always registered. Tab, Escape and the
//! next/previous shortcuts are registered only while a suggestion is visible,
//! so Mote never takes those keys from the user otherwise.

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use mote_core::engine::{EngineInput, ShortcutAction};
use mote_core::settings::KeyboardSettings;

use crate::error::CommandError;
use crate::state::AppState;

/// The shortcuts currently in effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutSet {
    pub palette: Shortcut,
    pub next: Shortcut,
    pub previous: Shortcut,
}

pub type SharedShortcuts = Arc<Mutex<ShortcutSet>>;

fn accept() -> Shortcut {
    Shortcut::new(None, Code::Tab)
}

fn dismiss() -> Shortcut {
    Shortcut::new(None, Code::Escape)
}

/// Parses an accelerator string; errors are user-facing.
pub fn parse(accelerator: &str) -> Result<Shortcut, CommandError> {
    Shortcut::from_str(accelerator)
        .map_err(|_| CommandError::invalid(format!("\"{accelerator}\" is not a supported shortcut.")))
}

impl ShortcutSet {
    pub fn from_settings(keyboard: &KeyboardSettings) -> Result<Self, CommandError> {
        Ok(Self {
            palette: parse(&keyboard.command_palette)?,
            next: parse(&keyboard.next_suggestion)?,
            previous: parse(&keyboard.previous_suggestion)?,
        })
    }
}

fn shortcuts(app: &AppHandle) -> Option<ShortcutSet> {
    app.try_state::<SharedShortcuts>().map(|s| s.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone())
}

/// Handles every registered global shortcut.
pub fn handle(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed {
        return;
    }
    let Some(set) = shortcuts(app) else { return };
    let action = if *shortcut == accept() {
        Some(ShortcutAction::Accept)
    } else if *shortcut == dismiss() {
        Some(ShortcutAction::Dismiss)
    } else if *shortcut == set.next {
        Some(ShortcutAction::Next)
    } else if *shortcut == set.previous {
        Some(ShortcutAction::Previous)
    } else {
        None
    };
    if let Some(action) = action {
        if let Some(state) = app.try_state::<Arc<AppState>>() {
            let _ = state.engine.tx.try_send(EngineInput::Shortcut(action));
        }
    } else if *shortcut == set.palette {
        crate::palette::open(app);
    }
}

/// Registers the palette shortcut at startup.
pub fn register_palette(app: &AppHandle) -> Result<(), CommandError> {
    let Some(set) = shortcuts(app) else { return Ok(()) };
    app.global_shortcut().register(set.palette).map_err(|error| {
        tracing::warn!(%error, "could not register the command palette shortcut");
        CommandError::new(
            "shortcut_unavailable",
            "That shortcut is already used by another application. Choose another.",
        )
    })
}

/// Replaces the shortcut set (e.g. after the user edits shortcuts).
pub fn rebind(app: &AppHandle, next: ShortcutSet) -> Result<(), CommandError> {
    let Some(shared) = app.try_state::<SharedShortcuts>() else { return Ok(()) };
    let previous = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    if previous.palette != next.palette {
        let manager = app.global_shortcut();
        let _ = manager.unregister(previous.palette);
        if let Err(error) = manager.register(next.palette) {
            tracing::warn!(%error, "new palette shortcut unavailable; restoring the previous one");
            let _ = manager.register(previous.palette);
            return Err(CommandError::new(
                "shortcut_unavailable",
                "That shortcut is already used by another application. Choose another.",
            ));
        }
    }
    *shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = next;
    Ok(())
}

pub fn register_suggestion_keys(app: &AppHandle) {
    let Some(set) = shortcuts(app) else { return };
    let manager = app.global_shortcut();
    for shortcut in [accept(), dismiss(), set.next, set.previous] {
        if let Err(error) = manager.register(shortcut) {
            tracing::debug!(%error, "suggestion shortcut unavailable");
        }
    }
}

pub fn unregister_suggestion_keys(app: &AppHandle) {
    let Some(set) = shortcuts(app) else { return };
    let manager = app.global_shortcut();
    for shortcut in [accept(), dismiss(), set.next, set.previous] {
        if manager.is_registered(shortcut) {
            let _ = manager.unregister(shortcut);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shortcuts_parse() {
        let set = ShortcutSet::from_settings(&KeyboardSettings::default()).unwrap();
        assert_ne!(set.palette, set.next);
        assert_ne!(set.next, set.previous);
        assert!(parse("Nonsense+Q").is_err());
    }
}
