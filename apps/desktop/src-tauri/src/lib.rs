//! Mote desktop application.

mod commands;
mod diagnostics;
mod error;
mod logging;
mod palette;
mod secrets;
mod settings_ops;
mod shell;
mod shortcuts;
mod state;
mod tray;
mod windows;
mod writers;

#[cfg(all(test, feature = "bindings"))]
mod bindings;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use tauri::{AppHandle, Manager, RunEvent};
use tokio::sync::watch;
use tracing_appender::non_blocking::WorkerGuard;

use mote_core::ai::{AiClient, Routing};
use mote_core::engine::{Engine, EngineDeps, EngineInput, EngineStatus};
use mote_core::observer::{Observer, ObserverConfig, OwnClipboardWrites};
use mote_core::privacy::PrivacyPolicy;
use mote_core::providers::resilient::ResilientProvider;
use mote_core::providers::ModelProvider;
use mote_core::settings::{KeyboardSettings, Settings};
use mote_providers::GroqProvider;
use mote_storage::Storage;

use crate::secrets::KeyringStore;
use crate::shell::DesktopShell;
use crate::shortcuts::{SharedShortcuts, ShortcutSet};
use crate::state::AppState;
use crate::writers::UsageWriter;

/// Keeps the log writer alive.
struct LogGuard(#[allow(dead_code)] Option<WorkerGuard>);

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = windows::show_main(app, None);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(shortcuts::handle).build())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            setup(app.handle()).map_err(|error| {
                tracing::error!(error = %error, "startup failed");
                error
            })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_settings,
            commands::save_settings,
            commands::get_provider_status,
            commands::set_api_key,
            commands::clear_api_key,
            commands::test_connection,
            commands::list_models,
            commands::get_usage_dashboard,
            commands::list_pricing,
            commands::save_pricing,
            commands::delete_pricing,
            commands::reset_pricing,
            commands::clear_usage_history,
            commands::clear_context,
            commands::reset_local_data,
            commands::list_exclusions,
            commands::add_exclusion,
            commands::remove_exclusion,
            commands::list_running_apps,
            commands::get_always_excluded,
            commands::get_recent_activity,
            commands::get_permission_status,
            commands::request_accessibility_permission,
            commands::open_accessibility_settings,
            commands::get_diagnostics,
            commands::copy_diagnostics,
            commands::set_paused,
            commands::complete_onboarding,
            commands::get_engine_status,
            commands::palette_context,
            commands::palette_run,
            commands::palette_cancel,
            commands::palette_apply,
            commands::palette_close,
            commands::palette_open_main,
            commands::overlay_ready,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Mote");

    app.run(|app, event| match event {
        RunEvent::ExitRequested { api, code, .. } => {
            let quitting = app.try_state::<Arc<AppState>>().is_some_and(|s| s.quitting.load(Ordering::SeqCst));
            // Closing the last window keeps Mote running in the menu bar / tray.
            if code.is_none() && !quitting {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => {
            if let Some(state) = app.try_state::<Arc<AppState>>() {
                state.set_observer(None);
                let _ = state.engine.tx.try_send(EngineInput::Shutdown);
            }
            tracing::info!("Mote stopped");
        }
        _ => {}
    });
}

fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    let log_dir = app.path().app_log_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&log_dir)?;
    app.manage(LogGuard(logging::init(&log_dir)));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Mote starting");

    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory)?;

    let storage = Storage::open(&data_dir.join("mote.db"))?;
    let mut settings = storage.load_settings()?.unwrap_or_default();
    if let Err(errors) = settings.validate() {
        tracing::warn!(count = errors.len(), "stored settings were invalid; using defaults");
        let onboarded = settings.general.onboarding_completed;
        settings = Settings::default();
        settings.general.onboarding_completed = onboarded;
    }
    if settings.general.paused_until.is_some_and(|until| until <= Utc::now()) {
        settings.general.paused_until = None;
    }

    let platform = mote_platform::create();
    let groq = Arc::new(GroqProvider::new(settings.provider.groq.base_url.clone(), None)?);
    let usage_writer =
        Arc::new(UsageWriter::spawn(storage.clone(), Some(app.clone()), settings.usage.analytics_enabled));
    let resilient = Arc::new(ResilientProvider::new(groq.clone(), usage_writer.clone()));
    let ai = Arc::new(AiClient::new(resilient.clone(), Routing::default()));
    let exclusions = storage.list_exclusions()?;
    let (policy_tx, policy_rx) = watch::channel(PrivacyPolicy::from_settings(&settings, exclusions));
    let retention = Arc::new(RwLock::new(settings.privacy.context_retention));
    let context_events = writers::spawn_context_writer(storage.clone(), retention.clone());
    let shell = Arc::new(DesktopShell::new(app.clone()));
    app.manage(shell.clone());
    let own_clipboard = Arc::new(OwnClipboardWrites::default());

    let (engine, engine_handle, engine_rx) = Engine::new(
        EngineDeps {
            platform: platform.clone(),
            ai: ai.clone(),
            shell: shell.clone(),
            own_clipboard_writes: own_clipboard.clone(),
            events: Some(context_events.clone()),
        },
        settings.clone(),
    );
    tauri::async_runtime::spawn(engine.run(engine_rx));

    let state = Arc::new(AppState {
        storage,
        secrets: Arc::new(KeyringStore::new(app.config().identifier.clone())),
        platform: platform.clone(),
        groq,
        resilient,
        ai,
        engine: engine_handle.clone(),
        settings: RwLock::new(settings.clone()),
        retention,
        policy: policy_tx,
        usage_writer,
        context_events,
        own_clipboard: own_clipboard.clone(),
        has_api_key: AtomicBool::new(false),
        key_hint: Mutex::new(None),
        last_health: Mutex::new(None),
        models_cache: Mutex::new(None),
        engine_status: Mutex::new(EngineStatus::default()),
        palette: Mutex::new(None),
        palette_cancel: Mutex::new(None),
        observer: Mutex::new(None),
        data_dir,
        log_dir,
        quitting: AtomicBool::new(false),
    });
    state.load_api_key();
    state.apply_settings(settings.clone());
    app.manage(state.clone());

    let shortcut_set = ShortcutSet::from_settings(&settings.keyboard)
        .or_else(|_| ShortcutSet::from_settings(&KeyboardSettings::default()))
        .map_err(|e| e.message)?;
    app.manage::<SharedShortcuts>(Arc::new(Mutex::new(shortcut_set)));
    if let Err(error) = shortcuts::register_palette(app) {
        tracing::warn!(%error, "command palette shortcut not registered");
    }

    windows::apply_theme(app, settings.general.theme);
    windows::create_overlay(app)?;
    tray::build(app, &settings)?;

    let observer =
        Observer::new(platform, policy_rx, engine_handle.tx.clone(), own_clipboard, ObserverConfig::default())
            .spawn()?;
    state.set_observer(Some(observer));

    spawn_maintenance(state.clone());
    spawn_health_check(state.clone());

    if !settings.general.onboarding_completed {
        windows::show_main(app, Some("onboarding"))?;
    }
    tracing::info!("Mote ready");
    Ok(())
}

/// Prunes context metadata and usage history according to retention settings.
fn spawn_maintenance(state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            let s = state.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let settings = s.settings();
                let now = Utc::now();
                let context_cutoff = settings.privacy.context_retention.as_duration().map_or(now, |d| now - d);
                match s.storage.prune_context(context_cutoff) {
                    Ok(n) if n > 0 => tracing::info!(removed = n, "pruned context metadata"),
                    Err(error) => tracing::warn!(%error, "context pruning failed"),
                    _ => {}
                }
                let usage_cutoff = now - chrono::Duration::days(i64::from(settings.usage.retention_days));
                if let Err(error) = s.storage.prune_usage(usage_cutoff) {
                    tracing::warn!(%error, "usage pruning failed");
                }
            })
            .await;
            tokio::time::sleep(Duration::from_secs(3_600)).await;
        }
    });
}

/// Verifies the stored API key and configured models shortly after launch.
fn spawn_health_check(state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        if !state.has_api_key() || !state.settings().privacy.cloud_ai_enabled {
            return;
        }
        let m = state.settings().provider.groq.models;
        let required = vec![m.completion, m.classification, m.writing, m.reasoning, m.fallback];
        let report = state.groq.health_check(&required).await;
        tracing::info!(ok = report.ok, missing = report.missing_models.len(), "provider health check");
        state.set_last_health(report);
    });
}
