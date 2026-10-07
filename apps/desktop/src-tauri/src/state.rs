//! Application state shared by IPC commands, the tray and the shell.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use mote_core::ai::{AiClient, Routing};
use mote_core::context::ContextEvent;
use mote_core::engine::{EngineHandle, EngineInput, EngineStatus};
use mote_core::observer::{ObserverHandle, OwnClipboardWrites};
use mote_core::platform::PlatformAdapter;
use mote_core::privacy::PrivacyPolicy;
use mote_core::providers::resilient::ResilientProvider;
use mote_core::providers::types::{HealthReport, ModelInfo};
use mote_core::settings::{RetentionPeriod, Settings};
use mote_providers::{ApiKey, GroqProvider};
use mote_storage::Storage;

use crate::palette::PaletteSession;
use crate::secrets::{SecretStore, GROQ_ACCOUNT};
use crate::writers::UsageWriter;

/// Everything the running app shares.
pub struct AppState {
    pub storage: Storage,
    pub secrets: Arc<dyn SecretStore>,
    pub platform: Arc<dyn PlatformAdapter>,
    pub groq: Arc<GroqProvider>,
    pub resilient: Arc<ResilientProvider>,
    pub ai: Arc<AiClient>,
    pub engine: EngineHandle,
    pub settings: RwLock<Settings>,
    pub retention: Arc<RwLock<RetentionPeriod>>,
    pub policy: watch::Sender<PrivacyPolicy>,
    pub usage_writer: Arc<UsageWriter>,
    pub context_events: mpsc::UnboundedSender<ContextEvent>,
    pub own_clipboard: Arc<OwnClipboardWrites>,
    pub has_api_key: AtomicBool,
    pub key_hint: Mutex<Option<String>>,
    pub last_health: Mutex<Option<HealthReport>>,
    pub models_cache: Mutex<Option<(Instant, Vec<ModelInfo>)>>,
    pub engine_status: Mutex<EngineStatus>,
    pub palette: Mutex<Option<PaletteSession>>,
    pub palette_cancel: Mutex<Option<CancellationToken>>,
    pub observer: Mutex<Option<ObserverHandle>>,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub quitting: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub fn has_api_key(&self) -> bool {
        self.has_api_key.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn routing(&self, settings: &Settings) -> Routing {
        Routing {
            models: settings.provider.groq.models.clone(),
            timeout: Duration::from_millis(u64::from(settings.provider.groq.request_timeout_ms)),
            cloud_enabled: settings.privacy.cloud_ai_enabled,
            configured: self.has_api_key(),
        }
    }

    /// Recomputes the privacy policy from settings and stored exclusions.
    pub fn refresh_policy(&self, settings: &Settings) {
        let exclusions = self.storage.list_exclusions().unwrap_or_else(|error| {
            tracing::warn!(%error, "could not load exclusions");
            Vec::new()
        });
        let _ = self.policy.send(PrivacyPolicy::from_settings(settings, exclusions));
    }

    /// Pushes new settings to every subsystem (does not persist them).
    pub fn apply_settings(&self, settings: Settings) {
        *self.settings.write().unwrap_or_else(std::sync::PoisonError::into_inner) = settings.clone();
        *self.retention.write().unwrap_or_else(std::sync::PoisonError::into_inner) = settings.privacy.context_retention;
        self.refresh_policy(&settings);
        self.groq.set_base_url(settings.provider.groq.base_url.clone());
        self.ai.set_routing(self.routing(&settings));
        self.usage_writer.set_enabled(settings.usage.analytics_enabled);
        let _ = self.engine.tx.try_send(EngineInput::Settings(Box::new(settings)));
    }

    /// Loads the API key from the credential store into the provider.
    pub fn load_api_key(&self) {
        match self.secrets.get(GROQ_ACCOUNT) {
            Ok(Some(key)) if !key.trim().is_empty() => {
                let key = ApiKey::new(key);
                *lock(&self.key_hint) = Some(key.hint());
                self.groq.set_api_key(Some(key));
                self.has_api_key.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            Ok(_) => {
                *lock(&self.key_hint) = None;
                self.groq.set_api_key(None);
                self.has_api_key.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            Err(error) => tracing::warn!(%error, "could not read the API key from the credential store"),
        }
        let settings = self.settings();
        self.ai.set_routing(self.routing(&settings));
    }

    pub fn key_hint(&self) -> Option<String> {
        lock(&self.key_hint).clone()
    }

    pub fn set_last_health(&self, report: HealthReport) {
        *lock(&self.last_health) = Some(report);
    }

    pub fn last_health(&self) -> Option<HealthReport> {
        lock(&self.last_health).clone()
    }

    pub fn engine_status(&self) -> EngineStatus {
        lock(&self.engine_status).clone()
    }

    pub fn set_engine_status(&self, status: EngineStatus) {
        *lock(&self.engine_status) = status;
    }

    pub fn palette(&self) -> std::sync::MutexGuard<'_, Option<PaletteSession>> {
        lock(&self.palette)
    }

    pub fn models_cache(&self) -> Option<Vec<ModelInfo>> {
        lock(&self.models_cache)
            .as_ref()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(300))
            .map(|(_, m)| m.clone())
    }

    pub fn set_models_cache(&self, models: Vec<ModelInfo>) {
        *lock(&self.models_cache) = Some((Instant::now(), models));
    }

    pub fn clear_models_cache(&self) {
        *lock(&self.models_cache) = None;
    }

    pub fn set_palette_cancel(&self, token: Option<CancellationToken>) {
        let mut slot = lock(&self.palette_cancel);
        if let Some(previous) = slot.take() {
            previous.cancel();
        }
        *slot = token;
    }

    pub fn set_observer(&self, handle: Option<ObserverHandle>) {
        *lock(&self.observer) = handle;
    }
}
