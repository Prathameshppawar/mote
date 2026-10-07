//! The observer polls the platform and feeds observations to the engine.
//!
//! Privacy is enforced here, *before* anything is read: when Mote is disabled,
//! paused, lacks permission, sees Secure Input, or the active application or
//! window is excluded, the observer reads no text and no clipboard content.
//! Polling adapts to activity (fast while typing, slow when idle) to keep CPU
//! and battery use low.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::{mpsc, watch};

use crate::assistance::ClipboardWriteLog;
use crate::context::manager::CLIPBOARD_MAX_CHARS;
use crate::engine::{EngineInput, Observation, UnobservableReason};
use crate::platform::{FocusedInput, PermissionState, PlatformAdapter, ReadLimits};
use crate::privacy::{ObservationDecision, PrivacyPolicy};
use crate::text::fnv1a64;

/// Clipboard sequence numbers produced by Mote's own writes (paste-to-insert).
#[derive(Debug, Default)]
pub struct OwnClipboardWrites {
    recent: Mutex<VecDeque<u64>>,
}

impl OwnClipboardWrites {
    pub fn contains(&self, sequence: u64) -> bool {
        self.recent.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&sequence)
    }
}

impl ClipboardWriteLog for OwnClipboardWrites {
    fn record_own_write(&self, sequence: u64) {
        let mut recent = self.recent.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        recent.push_back(sequence);
        while recent.len() > 16 {
            recent.pop_front();
        }
    }
}

/// Polling intervals.
#[derive(Debug, Clone, Copy)]
pub struct ObserverConfig {
    pub limits: ReadLimits,
    /// A text input changed within the last two seconds.
    pub active: Duration,
    /// A text input is focused but idle.
    pub focused: Duration,
    /// No text input is focused.
    pub idle: Duration,
    /// Observation is blocked (paused, excluded, no permission).
    pub blocked: Duration,
}

impl Default for ObserverConfig {
    fn default() -> Self {
        Self {
            limits: ReadLimits::default(),
            active: Duration::from_millis(100),
            focused: Duration::from_millis(250),
            idle: Duration::from_millis(500),
            blocked: Duration::from_millis(1_000),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FocusSignature {
    element_key: u64,
    before: u64,
    after: u64,
    selection: u64,
    caret: Option<(i64, i64)>,
}

impl FocusSignature {
    fn of(input: &FocusedInput) -> Self {
        Self {
            element_key: input.element_key,
            before: fnv1a64(input.text_before_caret.as_bytes()),
            after: fnv1a64(input.text_after_caret.as_bytes()),
            selection: input.selected_text.as_deref().map_or(0, |s| fnv1a64(s.as_bytes())),
            caret: input.caret_rect.map(|r| (r.x.round() as i64, r.y.round() as i64)),
        }
    }

    fn same_text(&self, other: &Self) -> bool {
        self.element_key == other.element_key && self.before == other.before && self.after == other.after
    }
}

/// Mutable observer state between ticks.
#[derive(Debug, Default)]
pub struct ObserverState {
    last_app: Option<(String, Option<String>, ObservationDecision)>,
    last_unobservable: Option<UnobservableReason>,
    last_focus: Option<FocusSignature>,
    focus_reported: bool,
    last_clipboard_sequence: Option<u64>,
    last_text_change: Option<Instant>,
}

/// Polls the platform on a dedicated thread.
pub struct Observer {
    platform: Arc<dyn PlatformAdapter>,
    policy: watch::Receiver<PrivacyPolicy>,
    tx: mpsc::Sender<EngineInput>,
    own_writes: Arc<OwnClipboardWrites>,
    config: ObserverConfig,
    stop: Arc<AtomicBool>,
}

/// Stops the observer thread when dropped.
pub struct ObserverHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ObserverHandle {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for ObserverHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Observer {
    pub fn new(
        platform: Arc<dyn PlatformAdapter>,
        policy: watch::Receiver<PrivacyPolicy>,
        tx: mpsc::Sender<EngineInput>,
        own_writes: Arc<OwnClipboardWrites>,
        config: ObserverConfig,
    ) -> Self {
        Self { platform, policy, tx, own_writes, config, stop: Arc::new(AtomicBool::new(false)) }
    }

    /// Starts polling on a background thread.
    pub fn spawn(self) -> std::io::Result<ObserverHandle> {
        let stop = self.stop.clone();
        let thread = std::thread::Builder::new().name("mote-observer".into()).spawn(move || {
            let mut state = ObserverState::default();
            while !self.stop.load(Ordering::SeqCst) {
                let interval = self.tick(&mut state);
                std::thread::sleep(interval);
            }
        })?;
        Ok(ObserverHandle { stop, thread: Some(thread) })
    }

    /// Sends an observation; returns false when it could not be delivered.
    fn send(&self, observation: Observation) -> bool {
        match self.tx.try_send(EngineInput::Observe(observation)) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => false,
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.stop.store(true, Ordering::SeqCst);
                false
            }
        }
    }

    fn report_unobservable(&self, state: &mut ObserverState, reason: UnobservableReason) {
        if state.last_unobservable != Some(reason) && self.send(Observation::Unobservable(reason)) {
            state.last_unobservable = Some(reason);
            state.last_app = None;
            state.last_focus = None;
            state.focus_reported = false;
        }
        // Content that appears on the clipboard while blocked is never read later.
        state.last_clipboard_sequence = Some(self.platform.clipboard_sequence());
    }

    /// One polling step. Returns how long to wait before the next one.
    pub fn tick(&self, state: &mut ObserverState) -> Duration {
        let policy = self.policy.borrow().clone();
        let now = Utc::now();
        if !policy.assistance_enabled || !policy.observe_applications {
            self.report_unobservable(state, UnobservableReason::Disabled);
            return self.config.blocked;
        }
        if policy.is_paused(now) {
            self.report_unobservable(state, UnobservableReason::Paused);
            return self.config.blocked;
        }
        let permissions = self.platform.permission_status();
        if permissions.secure_input_active {
            self.report_unobservable(state, UnobservableReason::SecureInput);
            return self.config.blocked;
        }
        if !matches!(permissions.accessibility, PermissionState::Granted | PermissionState::NotRequired) {
            self.report_unobservable(state, UnobservableReason::PermissionDenied);
            return self.config.blocked;
        }

        let Some(app) = self.platform.active_application() else {
            return self.config.idle;
        };
        let title = self.platform.active_window().and_then(|w| w.title);
        let decision = policy.evaluate(&app, title.as_deref(), now);
        let app_state = (app.id.clone(), title.clone(), decision);
        if state.last_app.as_ref() != Some(&app_state) || state.last_unobservable.is_some() {
            if !self.send(Observation::App { app: app.clone(), title, decision }) {
                return self.config.active;
            }
            state.last_app = Some(app_state);
            state.last_unobservable = None;
            state.last_focus = None;
            state.focus_reported = false;
        }

        let mut focused = false;
        if policy.may_read_text(decision) {
            match self.platform.focused_input(self.config.limits) {
                // The input must belong to the app the privacy decision was made
                // for; if focus moved to another app mid-poll, wait a tick.
                Ok(Some(input)) if !input.is_secure && input.app.id == app.id => {
                    focused = true;
                    let signature = FocusSignature::of(&input);
                    if state.last_focus != Some(signature) {
                        let text_changed = state.last_focus.is_none_or(|previous| !previous.same_text(&signature));
                        if self.send(Observation::Focus(Some(input))) {
                            state.last_focus = Some(signature);
                            state.focus_reported = true;
                            if text_changed {
                                state.last_text_change = Some(Instant::now());
                            }
                        }
                    }
                }
                _ => self.clear_focus(state),
            }
        } else {
            self.clear_focus(state);
        }

        let sequence = self.platform.clipboard_sequence();
        match state.last_clipboard_sequence {
            None => state.last_clipboard_sequence = Some(sequence),
            Some(previous) if previous != sequence => {
                state.last_clipboard_sequence = Some(sequence);
                if !self.own_writes.contains(sequence) {
                    if policy.may_read_clipboard(decision) {
                        if let Ok(Some(text)) = self.platform.clipboard_text(CLIPBOARD_MAX_CHARS) {
                            if !text.trim().is_empty() {
                                self.send(Observation::Clipboard { text, source_app: Some(app.clone()) });
                            }
                        }
                    } else {
                        self.send(Observation::ClipboardUnreadable);
                    }
                }
            }
            Some(_) => {}
        }

        if focused {
            if state.last_text_change.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
                self.config.active
            } else {
                self.config.focused
            }
        } else if decision.is_allowed() {
            self.config.idle
        } else {
            self.config.blocked
        }
    }

    fn clear_focus(&self, state: &mut ObserverState) {
        if (state.last_focus.is_some() || !state.focus_reported) && self.send(Observation::Focus(None)) {
            state.last_focus = None;
            state.focus_reported = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{AppInfo, PermissionStatus};
    use crate::privacy::{ExclusionKind, ExclusionReason, ExclusionRule};
    use crate::testing::{focused_input, FakePlatform};

    struct Rig {
        platform: Arc<FakePlatform>,
        policy_tx: watch::Sender<PrivacyPolicy>,
        rx: mpsc::Receiver<EngineInput>,
        observer: Observer,
        own: Arc<OwnClipboardWrites>,
        state: ObserverState,
    }

    fn rig() -> Rig {
        let platform = Arc::new(FakePlatform::default());
        let (policy_tx, policy_rx) = watch::channel(PrivacyPolicy::default());
        let (tx, rx) = mpsc::channel(64);
        let own = Arc::new(OwnClipboardWrites::default());
        let observer = Observer::new(platform.clone(), policy_rx, tx, own.clone(), ObserverConfig::default());
        Rig { platform, policy_tx, rx, observer, own, state: ObserverState::default() }
    }

    impl Rig {
        fn tick(&mut self) -> Vec<Observation> {
            self.observer.tick(&mut self.state);
            let mut out = Vec::new();
            while let Ok(EngineInput::Observe(o)) = self.rx.try_recv() {
                out.push(o);
            }
            out
        }
    }

    fn slack() -> AppInfo {
        AppInfo::new("com.tinyspeck.slackmacgap", "Slack")
    }

    #[test]
    fn reports_app_and_focus_changes_once() {
        let mut r = rig();
        r.platform.set_focus(Some(focused_input(slack(), 1, "hello")));
        let first = r.tick();
        assert!(matches!(first[0], Observation::App { decision: ObservationDecision::Allowed, .. }));
        assert!(matches!(&first[1], Observation::Focus(Some(f)) if f.text_before_caret == "hello"));
        assert!(r.tick().is_empty(), "nothing changed");
        r.platform.set_focus(Some(focused_input(slack(), 1, "hello there")));
        let next = r.tick();
        assert_eq!(next.len(), 1);
        assert!(matches!(&next[0], Observation::Focus(Some(f)) if f.text_before_caret == "hello there"));
    }

    #[test]
    fn excluded_apps_are_never_read() {
        let mut r = rig();
        let policy = PrivacyPolicy {
            exclusions: vec![ExclusionRule {
                id: 1,
                kind: ExclusionKind::App,
                pattern: "com.example.bank".into(),
                display_name: "Bank".into(),
            }],
            ..PrivacyPolicy::default()
        };
        r.policy_tx.send(policy).unwrap();
        r.platform.set_focus(Some(focused_input(AppInfo::new("com.example.bank", "Bank"), 1, "account 1234")));
        let obs = r.tick();
        assert!(matches!(
            obs[0],
            Observation::App { decision: ObservationDecision::Excluded(ExclusionReason::UserApp), .. }
        ));
        assert!(obs.iter().all(|o| !matches!(o, Observation::Focus(Some(_)))), "{obs:?}");
    }

    #[test]
    fn password_managers_are_never_read() {
        let mut r = rig();
        r.platform.set_focus(Some(focused_input(AppInfo::new("com.1password.1password", "1Password"), 1, "secret")));
        let obs = r.tick();
        assert!(obs.iter().all(|o| !matches!(o, Observation::Focus(Some(_)))));
    }

    #[test]
    fn input_from_a_different_app_than_the_checked_one_is_ignored() {
        let mut r = rig();
        // The platform reports a frontmost app, but the focused input belongs to another app
        // (the user switched between the two calls).
        r.platform.set_focus(Some(focused_input(AppInfo::new("com.example.bank", "Bank"), 1, "account 1234")));
        *r.platform.app.lock().unwrap() = Some(slack());
        let obs = r.tick();
        assert!(obs.iter().all(|o| !matches!(o, Observation::Focus(Some(_)))), "{obs:?}");
    }

    #[test]
    fn secure_fields_report_no_focus() {
        let mut r = rig();
        let mut input = focused_input(slack(), 1, "");
        input.is_secure = true;
        r.platform.set_focus(Some(input));
        let obs = r.tick();
        assert!(obs.iter().any(|o| matches!(o, Observation::Focus(None))));
    }

    #[test]
    fn pause_permission_and_secure_input_block_observation() {
        let mut r = rig();
        r.platform.set_focus(Some(focused_input(slack(), 1, "hello")));
        let policy =
            PrivacyPolicy { paused_until: Some(Utc::now() + chrono::Duration::hours(1)), ..PrivacyPolicy::default() };
        r.policy_tx.send(policy).unwrap();
        assert_eq!(r.tick(), vec![Observation::Unobservable(UnobservableReason::Paused)]);
        assert!(r.tick().is_empty(), "reported once");

        r.policy_tx.send(PrivacyPolicy::default()).unwrap();
        *r.platform.permission.lock().unwrap() = Some(PermissionStatus {
            accessibility: crate::platform::PermissionState::Denied,
            secure_input_active: false,
        });
        assert_eq!(r.tick(), vec![Observation::Unobservable(UnobservableReason::PermissionDenied)]);

        *r.platform.permission.lock().unwrap() = Some(PermissionStatus {
            accessibility: crate::platform::PermissionState::Granted,
            secure_input_active: true,
        });
        assert_eq!(r.tick(), vec![Observation::Unobservable(UnobservableReason::SecureInput)]);

        *r.platform.permission.lock().unwrap() = None;
        let resumed = r.tick();
        assert!(matches!(resumed[0], Observation::App { .. }), "observation resumes: {resumed:?}");
    }

    #[test]
    fn text_observation_switch_is_respected() {
        let mut r = rig();
        let policy = PrivacyPolicy { observe_text: false, ..PrivacyPolicy::default() };
        r.policy_tx.send(policy).unwrap();
        r.platform.set_focus(Some(focused_input(slack(), 1, "hello")));
        let obs = r.tick();
        assert!(obs.iter().all(|o| !matches!(o, Observation::Focus(Some(_)))));
    }

    #[test]
    fn clipboard_changes_are_reported_with_their_source() {
        let mut r = rig();
        r.platform.set_focus(Some(focused_input(slack(), 1, "x")));
        r.platform.set_clipboard_text("present before Mote started").unwrap();
        r.tick();
        r.platform.set_clipboard_text("The export fails on Safari for every customer").unwrap();
        let obs = r.tick();
        assert!(
            matches!(&obs[0], Observation::Clipboard { text, source_app: Some(a) } if text.starts_with("The export") && a.name == "Slack")
        );
        // Mote's own writes are ignored.
        let seq = r.platform.set_clipboard_text("inserted by Mote").unwrap();
        r.own.record_own_write(seq);
        assert!(r.tick().is_empty());
    }

    #[test]
    fn clipboard_copied_in_excluded_apps_is_never_read() {
        let mut r = rig();
        r.platform.set_focus(Some(focused_input(AppInfo::new("com.1password.1password", "1Password"), 1, "")));
        r.tick();
        r.platform.set_clipboard_text("hunter2").unwrap();
        let obs = r.tick();
        assert_eq!(obs, vec![Observation::ClipboardUnreadable]);
    }

    #[test]
    fn polling_slows_down_when_idle() {
        let mut r = rig();
        r.platform.set_focus(Some(focused_input(slack(), 1, "x")));
        let typing = r.observer.tick(&mut r.state);
        assert_eq!(typing, ObserverConfig::default().active);
        r.platform.set_focus(None);
        let idle = r.observer.tick(&mut r.state);
        assert_eq!(idle, ObserverConfig::default().idle);
        let policy = PrivacyPolicy { assistance_enabled: false, ..PrivacyPolicy::default() };
        r.policy_tx.send(policy).unwrap();
        assert_eq!(r.observer.tick(&mut r.state), ObserverConfig::default().blocked);
    }
}
