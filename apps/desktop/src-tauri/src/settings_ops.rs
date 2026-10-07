//! The single path through which settings change, whether from the settings
//! window, the tray or the palette.

use std::sync::Arc;

use chrono::{Duration, Utc};
use tauri::{AppHandle, Emitter};
use tauri_plugin_autostart::ManagerExt;

use mote_core::context::{ContextEvent, ContextEventKind};
use mote_core::settings::Settings;

use crate::error::{CommandError, CommandResult};
use crate::shortcuts::{self, ShortcutSet};
use crate::state::AppState;
use crate::tray;

/// Validates, persists and applies new settings.
pub fn save(app: &AppHandle, state: &Arc<AppState>, settings: Settings) -> CommandResult<Settings> {
    settings.validate().map_err(CommandError::validation)?;
    let previous = state.settings();
    if previous.keyboard != settings.keyboard {
        shortcuts::rebind(app, ShortcutSet::from_settings(&settings.keyboard)?)?;
    }
    if previous.general.launch_at_login != settings.general.launch_at_login {
        let autolaunch = app.autolaunch();
        let result = if settings.general.launch_at_login { autolaunch.enable() } else { autolaunch.disable() };
        if let Err(error) = result {
            tracing::warn!(%error, "could not change launch at login");
            return Err(CommandError::new("autostart", "Mote could not change the launch-at-login setting."));
        }
    }
    state.storage.save_settings(&settings)?;
    state.apply_settings(settings.clone());
    tray::refresh(app, &settings);
    let _ = app.emit("settings-changed", settings.clone());
    Ok(settings)
}

/// Pauses for `minutes` (or resumes with `None`).
pub fn set_paused(app: &AppHandle, state: &Arc<AppState>, minutes: Option<u32>) -> CommandResult<Settings> {
    let mut settings = state.settings();
    settings.general.paused_until = minutes.map(|m| Utc::now() + Duration::minutes(i64::from(m.clamp(1, 24 * 60))));
    let kind = match minutes {
        Some(m) => ContextEventKind::Paused { minutes: Some(m) },
        None => ContextEventKind::Resumed,
    };
    let saved = save(app, state, settings)?;
    let _ = state.context_events.send(ContextEvent { timestamp: Utc::now(), source: "mote".into(), kind });
    schedule_resume_refresh(app, &saved);
    Ok(saved)
}

/// When a pause ends on its own, refresh the tray so it stops saying "Resume".
fn schedule_resume_refresh(app: &AppHandle, settings: &Settings) {
    let Some(until) = settings.general.paused_until else { return };
    let Ok(wait) = (until - Utc::now()).to_std() else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(wait + std::time::Duration::from_secs(1)).await;
        if let Some(state) = tauri::Manager::try_state::<Arc<AppState>>(&app) {
            let settings = state.settings();
            if !settings.is_paused(Utc::now()) {
                tray::refresh(&app, &settings);
            }
        }
    });
}
