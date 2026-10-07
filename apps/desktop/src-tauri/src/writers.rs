//! Background persistence for usage events and context metadata.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use mote_core::context::ContextEvent;
use mote_core::settings::RetentionPeriod;
use mote_core::usage::{UsageEvent, UsageSink};
use mote_storage::Storage;

/// Records usage events on a dedicated thread so model calls never wait on
/// the database. Dropped silently when usage analytics are disabled.
pub struct UsageWriter {
    tx: mpsc::Sender<UsageEvent>,
    enabled: Arc<AtomicBool>,
}

impl UsageWriter {
    pub fn spawn(storage: Storage, app: Option<AppHandle>, enabled: bool) -> Self {
        let (tx, rx) = mpsc::channel::<UsageEvent>();
        let enabled = Arc::new(AtomicBool::new(enabled));
        let _ = std::thread::Builder::new().name("mote-usage-writer".into()).spawn(move || {
            let mut last_notify = Instant::now() - Duration::from_secs(10);
            while let Ok(event) = rx.recv() {
                if let Err(error) = storage.insert_usage_event(&event) {
                    tracing::warn!(%error, "could not record usage event");
                }
                if let Some(app) = &app {
                    if last_notify.elapsed() > Duration::from_secs(2) {
                        let _ = app.emit_to("main", "usage-updated", ());
                        last_notify = Instant::now();
                    }
                }
            }
        });
        Self { tx, enabled }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
}

impl UsageSink for UsageWriter {
    fn record(&self, event: UsageEvent) {
        if self.enabled.load(Ordering::SeqCst) {
            let _ = self.tx.send(event);
        }
    }
}

/// Persists context metadata events when the retention setting allows it.
pub fn spawn_context_writer(
    storage: Storage,
    retention: Arc<std::sync::RwLock<RetentionPeriod>>,
) -> tokio::sync::mpsc::UnboundedSender<ContextEvent> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ContextEvent>();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            let keep = *retention.read().unwrap_or_else(std::sync::PoisonError::into_inner) != RetentionPeriod::Off;
            if keep {
                let storage = storage.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    if let Err(error) = storage.record_context_event(&event) {
                        tracing::warn!(%error, "could not record context event");
                    }
                })
                .await;
            }
        }
    });
    tx
}
