//! The desktop implementation of the engine's UI shell: overlay window,
//! suggestion shortcuts and status propagation.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use mote_core::engine::{AssistantShell, EngineStatus, OverlayKind, OverlayView};
use mote_core::platform::Rect;

use crate::shortcuts;
use crate::state::AppState;
use crate::tray;
use crate::windows::{self, clamp_to_monitor, overlay_origin, place_overlay, Placement};

/// Payload for the overlay window.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct OverlayPayload {
    pub seq: u64,
    pub view: OverlayView,
}

pub struct DesktopShell {
    app: AppHandle,
    seq: AtomicU64,
    current: Mutex<Option<OverlayView>>,
    keys_active: Mutex<bool>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl DesktopShell {
    pub fn new(app: AppHandle) -> Self {
        Self { app, seq: AtomicU64::new(0), current: Mutex::new(None), keys_active: Mutex::new(false) }
    }

    fn placement(kind: OverlayKind) -> Placement {
        match kind {
            OverlayKind::Completion => Placement::Inline,
            _ => Placement::Below,
        }
    }

    /// Fallback anchor when the app does not expose a caret: near the pointer.
    fn fallback_anchor(&self, view: &OverlayView) -> Option<Rect> {
        let cursor = self.app.cursor_position().ok()?;
        let scale = match view.coordinate_space {
            mote_core::platform::CoordinateSpace::LogicalPoints => {
                self.app.monitor_from_point(cursor.x, cursor.y).ok().flatten().map_or(1.0, |m| m.scale_factor())
            }
            mote_core::platform::CoordinateSpace::PhysicalPixels => 1.0,
        };
        Some(Rect { x: cursor.x / scale + 12.0, y: cursor.y / scale + 12.0, width: 1.0, height: 18.0 })
    }

    /// Positions and shows the overlay for `view` at the given size.
    fn present(&self, view: &OverlayView, width: f64, height: f64) {
        let Some(window) = self.app.get_webview_window(windows::OVERLAY) else { return };
        let Some(anchor) = view.anchor.or_else(|| self.fallback_anchor(view)) else { return };
        let (x, y) = overlay_origin(anchor, Self::placement(view.kind), height);
        let (x, y) = clamp_to_monitor(&self.app, view.coordinate_space, x, y, width, height);
        if let Err(error) = place_overlay(&window, view.coordinate_space, x, y, width, height) {
            tracing::debug!(%error, "could not place overlay");
        }
        if !window.is_visible().unwrap_or(false) {
            let _ = window.show();
        }
    }

    /// Called when the overlay reports the size of its rendered content.
    pub fn on_overlay_ready(&self, seq: u64, width: f64, height: f64) {
        if seq != self.seq.load(Ordering::SeqCst) {
            return;
        }
        if let Some(view) = lock(&self.current).clone() {
            self.present(&view, width.clamp(40.0, 720.0), height.clamp(20.0, 240.0));
        }
    }

    /// Shows a short notice (e.g. "Copied to clipboard") near the pointer.
    pub fn notice(&self, text: &str) {
        let view = OverlayView {
            kind: OverlayKind::Notice,
            text: text.to_string(),
            detail: None,
            index: 0,
            count: 1,
            anchor: None,
            coordinate_space: mote_core::platform::CoordinateSpace::LogicalPoints,
            accept_hint: None,
        };
        self.show(&view);
        let app = self.app.clone();
        let seq = self.seq.load(Ordering::SeqCst);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if let Some(shell) = app.try_state::<std::sync::Arc<DesktopShell>>() {
                if shell.seq.load(Ordering::SeqCst) == seq {
                    shell.hide();
                }
            }
        });
    }
}

impl AssistantShell for DesktopShell {
    fn show(&self, view: &OverlayView) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        *lock(&self.current) = Some(view.clone());
        let _ = self.app.emit_to(windows::OVERLAY, "overlay-view", OverlayPayload { seq, view: view.clone() });
        // Show immediately with an estimate; the overlay refines the size.
        let chars = view.text.chars().count() + view.detail.as_ref().map_or(0, |d| d.chars().count());
        let width = (chars as f64 * 7.6 + 72.0).clamp(80.0, 640.0);
        let height = if view.kind == OverlayKind::Completion { 32.0 } else { 40.0 };
        self.present(view, width, height);
    }

    fn hide(&self) {
        self.seq.fetch_add(1, Ordering::SeqCst);
        *lock(&self.current) = None;
        if let Some(window) = self.app.get_webview_window(windows::OVERLAY) {
            let _ = window.hide();
        }
        let _ = self.app.emit_to(windows::OVERLAY, "overlay-hide", ());
    }

    fn set_suggestion_keys(&self, active: bool) {
        let mut current = lock(&self.keys_active);
        if *current == active {
            return;
        }
        *current = active;
        let app = self.app.clone();
        if active {
            shortcuts::register_suggestion_keys(&app);
        } else {
            shortcuts::unregister_suggestion_keys(&app);
        }
    }

    fn status(&self, status: &EngineStatus) {
        if let Some(state) = self.app.try_state::<std::sync::Arc<AppState>>() {
            state.set_engine_status(status.clone());
        }
        tray::update_status(&self.app, status);
        let _ = self.app.emit("engine-status", status.clone());
    }
}
