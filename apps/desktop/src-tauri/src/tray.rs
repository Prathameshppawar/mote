//! The menu-bar (macOS) / system-tray (Windows) menu.

use std::sync::Arc;

use chrono::{Local, Utc};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use mote_core::engine::{shortcut_label, EngineState, EngineStatus};
use mote_core::settings::Settings;

use crate::settings_ops;
use crate::state::AppState;
use crate::windows;

pub const TRAY_ID: &str = "mote";

/// Menu items whose state changes at runtime.
pub struct TrayMenu {
    status: MenuItem<Wry>,
    assistance: CheckMenuItem<Wry>,
    completion: CheckMenuItem<Wry>,
    context: CheckMenuItem<Wry>,
    groq: CheckMenuItem<Wry>,
    palette: MenuItem<Wry>,
    pause: MenuItem<Wry>,
    update: MenuItem<Wry>,
}

#[cfg(target_os = "macos")]
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");
#[cfg(not(target_os = "macos"))]
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");

pub fn build(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "Starting…", true, None::<&str>)?;
    let assistance = CheckMenuItem::with_id(app, "toggle-assistance", "Enable assistance", true, false, None::<&str>)?;
    let completion = CheckMenuItem::with_id(app, "toggle-completion", "Enable completion", true, false, None::<&str>)?;
    let context = CheckMenuItem::with_id(app, "toggle-context", "Context awareness", true, false, None::<&str>)?;
    let groq = CheckMenuItem::with_id(app, "provider-groq", "Groq", true, true, None::<&str>)?;
    let provider = Submenu::with_items(app, "AI Provider", true, &[&groq.clone()])?;
    let palette = MenuItem::with_id(app, "palette", "Command Palette", true, None::<&str>)?;
    let usage = MenuItem::with_id(app, "usage", "Usage", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let privacy = MenuItem::with_id(app, "privacy", "Privacy", true, None::<&str>)?;
    let diagnostics = MenuItem::with_id(app, "diagnostics", "Diagnostics", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause for 1 hour", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Mote", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Check for Updates…", true, None::<&str>)?;
    let title = MenuItem::with_id(app, "title", "Mote", false, None::<&str>)?;
    let separator = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(
        app,
        &[
            &title,
            &status,
            &separator()?,
            &assistance,
            &completion,
            &context,
            &separator()?,
            &provider,
            &separator()?,
            &palette,
            &usage,
            &settings_item,
            &privacy,
            &diagnostics,
            &update,
            &separator()?,
            &pause,
            &quit,
        ],
    )?;
    app.manage(TrayMenu { status, assistance, completion, context, groq, palette, pause, update });
    refresh(app, settings);

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Mote")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;
    Ok(())
}

/// Updates check marks and the pause item from settings.
pub fn refresh(app: &AppHandle, settings: &Settings) {
    let Some(menu) = app.try_state::<TrayMenu>() else { return };
    let _ = menu.assistance.set_checked(settings.general.assistance_enabled);
    let _ = menu.completion.set_checked(settings.completion.enabled);
    let _ = menu.context.set_checked(settings.context.contextual_suggestions && settings.privacy.observe_clipboard);
    let _ = menu.palette.set_text(format!("Command Palette    {}", shortcut_label(&settings.keyboard.command_palette)));
    let paused = settings.is_paused(Utc::now());
    let _ = menu.pause.set_text(if paused { "Resume" } else { "Pause for 1 hour" });
}

/// A verified update is downloaded: offer to restart into it.
pub fn update_ready(app: &AppHandle, version: &str) {
    if let Some(menu) = app.try_state::<TrayMenu>() {
        let _ = menu.update.set_text(format!("Restart to Update to {version}"));
    }
}

/// Updates the status line and tooltip.
pub fn update_status(app: &AppHandle, status: &EngineStatus) {
    let paused_until = app
        .try_state::<Arc<AppState>>()
        .and_then(|s| s.settings().general.paused_until)
        .map(|t| t.with_timezone(&Local).format("%H:%M").to_string());
    let text = match status.state {
        EngineState::Starting => "Starting…".to_string(),
        EngineState::Active | EngineState::Idle => "Active ✓".to_string(),
        EngineState::Paused => match paused_until {
            Some(time) => format!("Paused until {time}"),
            None => "Paused".to_string(),
        },
        EngineState::Disabled => "Assistance is off".to_string(),
        EngineState::Excluded => "Excluded app: not observing".to_string(),
        EngineState::NeedsPermission => "Grant Accessibility permission…".to_string(),
        EngineState::SecureInput => "Secure input active: paused".to_string(),
        EngineState::NeedsApiKey => "Add your Groq API key…".to_string(),
        EngineState::CloudDisabled => "Cloud AI is off (Privacy)".to_string(),
        EngineState::Offline => "Offline: cloud assistance paused".to_string(),
        EngineState::RateLimited => "Rate limited: retrying soon".to_string(),
    };
    if let Some(menu) = app.try_state::<TrayMenu>() {
        let _ = menu.status.set_text(&text);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(format!("Mote: {text}")));
    }
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let Some(state) = app.try_state::<Arc<AppState>>() else { return };
    let state = state.inner().clone();
    match event.id().as_ref() {
        "status" => {
            let section = match state.engine_status().state {
                EngineState::NeedsPermission => Some("onboarding"),
                EngineState::NeedsApiKey => Some("providers"),
                EngineState::CloudDisabled | EngineState::Excluded => Some("privacy"),
                EngineState::Disabled | EngineState::Paused => Some("general"),
                _ => Some("diagnostics"),
            };
            let _ = windows::show_main(app, section);
        }
        "toggle-assistance" => toggle(app, |s| s.general.assistance_enabled = !s.general.assistance_enabled),
        "toggle-completion" => toggle(app, |s| s.completion.enabled = !s.completion.enabled),
        "toggle-context" => toggle(app, |s| {
            let on = !(s.context.contextual_suggestions && s.privacy.observe_clipboard);
            s.context.contextual_suggestions = on;
            s.privacy.observe_clipboard = on;
        }),
        "provider-groq" => {
            // Groq is the only provider in v1: keep it checked and open its settings.
            if let Some(menu) = app.try_state::<TrayMenu>() {
                let _ = menu.groq.set_checked(true);
            }
            let _ = windows::show_main(app, Some("providers"));
        }
        "palette" => crate::palette::open(app),
        "usage" => {
            let _ = windows::show_main(app, Some("usage"));
        }
        "settings" => {
            let _ = windows::show_main(app, Some("general"));
        }
        "privacy" => {
            let _ = windows::show_main(app, Some("privacy"));
        }
        "diagnostics" => {
            let _ = windows::show_main(app, Some("diagnostics"));
        }
        "pause" => {
            let paused = state.settings().is_paused(Utc::now());
            let minutes = if paused { None } else { Some(60) };
            if let Err(error) = settings_ops::set_paused(app, &state, minutes) {
                tracing::warn!(%error, "could not change pause state");
            }
        }
        "update" => {
            let Some(updates) = app.try_state::<Arc<crate::updates::Updates>>() else { return };
            let updates = updates.inner().clone();
            if matches!(updates.status(app).state, crate::updates::UpdateState::Ready { .. }) {
                if let Err(error) = updates.install_and_restart(app) {
                    tracing::warn!(%error, "could not install the update");
                }
            } else {
                let _ = windows::show_main(app, Some("about"));
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    updates.check(&app).await;
                });
            }
        }
        "quit" => {
            state.quitting.store(true, std::sync::atomic::Ordering::SeqCst);
            app.exit(0);
        }
        _ => {}
    }
}

fn toggle(app: &AppHandle, change: impl FnOnce(&mut Settings)) {
    let Some(state) = app.try_state::<Arc<AppState>>() else { return };
    let mut settings = state.settings();
    change(&mut settings);
    if let Err(error) = settings_ops::save(app, &state, settings) {
        tracing::warn!(%error, "could not save settings from the tray");
    }
}
