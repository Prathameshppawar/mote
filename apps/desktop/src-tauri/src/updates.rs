//! In-app updates from GitHub Releases.
//!
//! Shortly after launch and then every six hours (when enabled), Mote checks
//! the latest release, downloads a newer version in the background and
//! verifies its signature against the public key built into the app. Nothing
//! is installed until the user chooses "Restart to update" (tray or Settings →
//! About). The check sends no personal data: it is a plain download of the
//! release manifest.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use crate::tray;

const FIRST_CHECK_AFTER: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// Where the updater stands.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum UpdateState {
    /// No check yet this session.
    Idle,
    Checking,
    UpToDate,
    Downloading {
        version: String,
    },
    /// Downloaded and verified; installs on restart.
    Ready {
        version: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub state: UpdateState,
}

/// Updater state shared by the background task, commands and the tray.
pub struct Updates {
    state: Mutex<UpdateState>,
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    busy: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Updates {
    pub fn new() -> Self {
        Self { state: Mutex::new(UpdateState::Idle), ready: Mutex::new(None), busy: AtomicBool::new(false) }
    }

    pub fn status(&self, app: &AppHandle) -> UpdateStatus {
        UpdateStatus { current_version: app.package_info().version.to_string(), state: lock(&self.state).clone() }
    }

    fn set(&self, app: &AppHandle, state: UpdateState) {
        *lock(&self.state) = state.clone();
        let _ = app.emit("update-status", self.status(app));
        if let UpdateState::Ready { version } = &state {
            tray::update_ready(app, version);
        }
    }

    /// Checks for a newer release and downloads it. Returns the resulting status.
    pub async fn check(&self, app: &AppHandle) -> UpdateStatus {
        if self.busy.swap(true, Ordering::SeqCst) {
            return self.status(app);
        }
        let outcome = self.check_inner(app).await;
        self.busy.store(false, Ordering::SeqCst);
        match outcome {
            Ok(state) => self.set(app, state),
            Err(message) => {
                tracing::info!(%message, "update check failed");
                self.set(app, UpdateState::Failed { message });
            }
        }
        self.status(app)
    }

    async fn check_inner(&self, app: &AppHandle) -> Result<UpdateState, String> {
        if let Some((update, _)) = lock(&self.ready).as_ref() {
            return Ok(UpdateState::Ready { version: update.version.clone() });
        }
        self.set(app, UpdateState::Checking);
        let updater = app.updater().map_err(|e| describe(&e))?;
        let Some(update) = updater.check().await.map_err(|e| describe(&e))? else {
            return Ok(UpdateState::UpToDate);
        };
        let version = update.version.clone();
        tracing::info!(%version, "update available; downloading");
        self.set(app, UpdateState::Downloading { version: version.clone() });
        let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| describe(&e))?;
        *lock(&self.ready) = Some((update, bytes));
        tracing::info!(%version, "update downloaded and verified");
        Ok(UpdateState::Ready { version })
    }

    /// Installs the downloaded update and restarts Mote.
    pub fn install_and_restart(&self, app: &AppHandle) -> CommandResult<()> {
        let Some((update, bytes)) = lock(&self.ready).take() else {
            return Err(CommandError::invalid("There is no downloaded update to install."));
        };
        if let Some(state) = app.try_state::<Arc<AppState>>() {
            state.quitting.store(true, Ordering::SeqCst);
        }
        if let Err(error) = update.install(bytes) {
            tracing::warn!(error = %describe(&error), "update install failed");
            self.set(app, UpdateState::Failed { message: describe(&error) });
            return Err(CommandError::new("update", "Mote could not install the update. It will try again later."));
        }
        tracing::info!(version = %update.version, "update installed; restarting");
        app.restart();
    }
}

/// A short, user-facing description of an updater error (no URLs or keys).
fn describe(error: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error as E;
    match error {
        E::Reqwest(_) | E::Network(_) => "Couldn't reach GitHub to check for updates.".into(),
        E::Minisign(_) | E::SignatureUtf8(_) | E::Base64(_) => {
            "The downloaded update failed its signature check and was discarded.".into()
        }
        E::ReleaseNotFound => "No published release was found.".into(),
        other => mote_core::privacy::redact::redact_secrets(&other.to_string()).chars().take(200).collect(),
    }
}

/// Checks after launch and then periodically, while automatic updates are on.
pub fn spawn_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        loop {
            let enabled = app.try_state::<Arc<AppState>>().is_some_and(|s| s.settings().general.auto_update);
            if enabled {
                if let Some(updates) = app.try_state::<Arc<Updates>>() {
                    updates.check(&app).await;
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}
