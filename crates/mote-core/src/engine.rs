//! The assistance engine.
//!
//! A single async task that receives observations and shortcut presses,
//! maintains the context window, classifies intent, schedules debounced
//! completions and writing checks, and drives the overlay through
//! [`AssistantShell`]. Model calls run in spawned tasks and report back through
//! the engine's inbox, so typing is never blocked and stale results (for text
//! the user has since changed) are simply discarded.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::ai::AiClient;
use crate::assistance::{
    apply_edit, correction_from_ai, grammar_candidate, spelling_correction, writing_applies, ClipboardWriteLog,
    Correction, EditPlan,
};
use crate::completion::{
    anchor_key, should_complete, CompletionCache, DismissedAnchors, SkipReason, SuggestionSet, SuggestionUpdate,
    COMPLETION_CONTEXT_CHARS,
};
use crate::context::insights::{self, ContextSuggestion};
use crate::context::manager::ClipboardSnapshot;
use crate::context::{ContextEvent, ContextEventKind, ContextManager};
use crate::intent::apps::categorize;
use crate::intent::{self, IntentAssessment, IntentKind, IntentSignals, IntentSource, IntentSubtype};
use crate::language::{detect, LanguageLabel, LanguageProfile};
use crate::platform::{AppInfo, CoordinateSpace, FocusedInput, Key, PlatformAdapter, PlatformError, ReadLimits, Rect};
use crate::privacy::ObservationDecision;
use crate::prompts::{ClassificationPrompt, CompletionPrompt};
use crate::providers::types::Feature;
use crate::providers::ProviderError;
use crate::settings::Settings;
use crate::text::{fnv1a64, tail_at_word_boundary, tail_chars};

/// Something the observer saw.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// The frontmost application, its (in-memory) window title and whether Mote may observe it.
    App { app: AppInfo, title: Option<String>, decision: ObservationDecision },
    /// Mote cannot or may not observe anything right now.
    Unobservable(UnobservableReason),
    /// The focused text input changed (`None`: no text input has focus).
    Focus(Option<FocusedInput>),
    /// The clipboard changed and may be read.
    Clipboard { text: String, source_app: Option<AppInfo> },
    /// The clipboard changed but may not be read (excluded app): forget the old copy.
    ClipboardUnreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum UnobservableReason {
    Disabled,
    Paused,
    PermissionDenied,
    SecureInput,
    Excluded,
}

/// Keyboard actions while a suggestion is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    Accept,
    Dismiss,
    Next,
    Previous,
}

/// Messages to the engine.
#[derive(Debug)]
pub enum EngineInput {
    Observe(Observation),
    Shortcut(ShortcutAction),
    Settings(Box<Settings>),
    ClearContext,
    Shutdown,
    CompletionReady { id: u64, result: Result<Option<String>, ProviderError> },
    ClassificationReady { id: u64, element_key: u64, result: Result<Option<IntentAssessment>, ProviderError> },
    GrammarReady { id: u64, result: Result<Option<String>, ProviderError> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OverlayKind {
    Completion,
    Correction,
    PromptHint,
    Context,
    /// A transient message from the app (e.g. "Copied to clipboard").
    Notice,
}

/// What the overlay should display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct OverlayView {
    pub kind: OverlayKind,
    /// Completion remainder, corrected text, or chip title.
    pub text: String,
    /// "completd → completed", or context actions.
    pub detail: Option<String>,
    pub index: u32,
    pub count: u32,
    /// Caret rectangle in the platform's coordinate space.
    pub anchor: Option<Rect>,
    #[cfg_attr(feature = "ts", ts(type = "\"logical_points\" | \"physical_pixels\""))]
    pub coordinate_space: CoordinateSpace,
    /// Key hint such as "Tab".
    pub accept_hint: Option<String>,
}

/// The engine's externally visible state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    Starting,
    /// Watching a text input.
    Active,
    /// Running; no text input focused.
    Idle,
    Paused,
    Disabled,
    Excluded,
    NeedsPermission,
    SecureInput,
    NeedsApiKey,
    CloudDisabled,
    Offline,
    RateLimited,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub state: EngineState,
    pub message: Option<String>,
    pub app: Option<String>,
    pub intent: Option<IntentKind>,
    pub subtype: Option<IntentSubtype>,
    pub intent_confidence: Option<f32>,
    pub language: Option<LanguageLabel>,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            state: EngineState::Starting,
            message: None,
            app: None,
            intent: None,
            subtype: None,
            intent_confidence: None,
            language: None,
        }
    }
}

/// The UI side of the engine: overlay, suggestion shortcuts and status.
pub trait AssistantShell: Send + Sync {
    fn show(&self, view: &OverlayView);
    fn hide(&self);
    /// Registers (true) or releases (false) Tab, Escape and next/previous.
    fn set_suggestion_keys(&self, active: bool);
    fn status(&self, status: &EngineStatus);
}

/// What the command palette needs to know about the user's context, captured
/// before the palette takes focus. In memory only.
#[derive(Debug, Clone, Default)]
pub struct EngineSnapshot {
    pub app: Option<AppInfo>,
    pub focus: Option<FocusedInput>,
    pub intent: Option<IntentAssessment>,
    pub language: Option<LanguageProfile>,
    pub clipboard: Option<ClipboardSnapshot>,
    pub context_suggestion: Option<ContextSuggestion>,
    pub status: EngineStatus,
}

/// Dependencies injected into the engine.
pub struct EngineDeps {
    pub platform: Arc<dyn PlatformAdapter>,
    pub ai: Arc<AiClient>,
    pub shell: Arc<dyn AssistantShell>,
    pub own_clipboard_writes: Arc<dyn ClipboardWriteLog>,
    /// Metadata events for persistence (the receiver applies retention).
    pub events: Option<mpsc::UnboundedSender<ContextEvent>>,
}

/// Handle used by the rest of the app to talk to the engine.
#[derive(Clone)]
pub struct EngineHandle {
    pub tx: mpsc::Sender<EngineInput>,
    pub snapshot: Arc<Mutex<EngineSnapshot>>,
}

impl EngineHandle {
    pub fn snapshot(&self) -> EngineSnapshot {
        self.snapshot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

const WRITING_DELAY_AFTER_BOUNDARY: Duration = Duration::from_millis(700);
const WRITING_DELAY_MID_WORD: Duration = Duration::from_millis(1_600);
const HINT_DELAY: Duration = Duration::from_millis(2_000);
const CHIP_LIFETIME: Duration = Duration::from_secs(8);
const MIN_HINT_CHARS: usize = 15;
const TAB_LOOP_GUARD: Duration = Duration::from_millis(250);
const MAX_ALTERNATIVES: usize = 3;
const CLASSIFICATION_EXCERPT_CHARS: usize = 400;

enum Active {
    Completion(SuggestionSet),
    Correction(Correction),
    Hint,
    Context(ContextSuggestion),
}

struct PendingCompletion {
    id: u64,
    cancel: CancellationToken,
    anchor: String,
    element_key: u64,
    key: u64,
    alternative: bool,
}

struct PendingGrammar {
    id: u64,
    cancel: CancellationToken,
    sentence: String,
    element_key: u64,
}

pub struct Engine {
    deps: EngineDeps,
    tx: mpsc::Sender<EngineInput>,
    snapshot: Arc<Mutex<EngineSnapshot>>,
    settings: Settings,
    context: ContextManager,
    unobservable: Option<UnobservableReason>,
    focus: Option<FocusedInput>,
    language: LanguageProfile,
    intent: Option<IntentAssessment>,
    ai_intents: HashMap<u64, IntentAssessment>,
    classification_pending: Option<(u64, u64)>,
    active: Option<Active>,
    pending_completion: Option<PendingCompletion>,
    pending_grammar: Option<PendingGrammar>,
    completion_due: Option<Instant>,
    writing_due: Option<Instant>,
    hint_due: Option<Instant>,
    chip_expires: Option<Instant>,
    last_completion_request: Option<Instant>,
    next_id: u64,
    cache: CompletionCache,
    dismissed: DismissedAnchors,
    checked: HashSet<u64>,
    hinted: HashSet<u64>,
    offered_clipboard: Option<chrono::DateTime<Utc>>,
    ignored_words: HashSet<String>,
    provider_problem: Option<(EngineState, String)>,
    last_status: Option<EngineStatus>,
    last_tab_passthrough: Option<Instant>,
}

impl Engine {
    /// Creates the engine and its handle. Run it with [`Engine::run`].
    pub fn new(deps: EngineDeps, settings: Settings) -> (Self, EngineHandle, mpsc::Receiver<EngineInput>) {
        let (tx, rx) = mpsc::channel(256);
        let snapshot = Arc::new(Mutex::new(EngineSnapshot::default()));
        let ttl = Duration::from_secs(u64::from(settings.context.clipboard_ttl_secs));
        let ignored_words = settings.writing.ignored_words.iter().map(|w| w.to_lowercase()).collect();
        let engine = Self {
            context: ContextManager::new(deps.platform.os(), ttl),
            deps,
            tx: tx.clone(),
            snapshot: snapshot.clone(),
            settings,
            unobservable: None,
            focus: None,
            language: LanguageProfile::unknown(),
            intent: None,
            ai_intents: HashMap::new(),
            classification_pending: None,
            active: None,
            pending_completion: None,
            pending_grammar: None,
            completion_due: None,
            writing_due: None,
            hint_due: None,
            chip_expires: None,
            last_completion_request: None,
            next_id: 0,
            cache: CompletionCache::new(64, Duration::from_secs(600)),
            dismissed: DismissedAnchors::default(),
            checked: HashSet::new(),
            hinted: HashSet::new(),
            offered_clipboard: None,
            ignored_words,
            provider_problem: None,
            last_status: None,
            last_tab_passthrough: None,
        };
        (engine, EngineHandle { tx, snapshot }, rx)
    }

    /// Runs until [`EngineInput::Shutdown`] or until all senders are dropped.
    pub async fn run(mut self, mut rx: mpsc::Receiver<EngineInput>) {
        self.publish_status();
        loop {
            let deadline =
                [self.completion_due, self.writing_due, self.hint_due, self.chip_expires].into_iter().flatten().min();
            tokio::select! {
                input = rx.recv() => match input {
                    None | Some(EngineInput::Shutdown) => break,
                    Some(input) => self.handle(input).await,
                },
                _ = sleep_until(deadline) => self.on_timer().await,
            }
            self.update_snapshot();
        }
        self.cancel_pending();
        self.hide();
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn emit(&self, events: Vec<ContextEvent>) {
        if let Some(tx) = &self.deps.events {
            for event in events {
                let _ = tx.send(event);
            }
        }
    }

    fn record(&mut self, kind: ContextEventKind) {
        let event = self.context.record(kind, Utc::now());
        self.emit(vec![event]);
    }

    async fn handle(&mut self, input: EngineInput) {
        match input {
            EngineInput::Observe(observation) => self.observe(observation),
            EngineInput::Shortcut(action) => self.shortcut(action).await,
            EngineInput::Settings(settings) => self.apply_settings(*settings),
            EngineInput::ClearContext => {
                self.context.clear();
                self.cache.clear();
                self.dismissed.clear();
                self.checked.clear();
                self.ai_intents.clear();
                self.hinted.clear();
                self.offered_clipboard = None;
                self.reset_focus();
            }
            EngineInput::Shutdown => {}
            EngineInput::CompletionReady { id, result } => self.on_completion(id, result),
            EngineInput::ClassificationReady { id, element_key, result } => {
                self.on_classification(id, element_key, result)
            }
            EngineInput::GrammarReady { id, result } => self.on_grammar(id, result),
        }
        self.publish_status();
    }

    fn apply_settings(&mut self, settings: Settings) {
        self.context.set_clipboard_ttl(Duration::from_secs(u64::from(settings.context.clipboard_ttl_secs)));
        self.ignored_words = settings.writing.ignored_words.iter().map(|w| w.to_lowercase()).collect();
        let assistance_off = !settings.general.assistance_enabled;
        self.settings = settings;
        if assistance_off {
            self.reset_focus();
        }
        self.provider_problem = None;
    }

    fn observe(&mut self, observation: Observation) {
        let now = Utc::now();
        match observation {
            Observation::App { app, title, decision } => {
                if decision.is_allowed() {
                    self.unobservable = None;
                    let switched = self.context.active_app().is_none_or(|current| current.id != app.id);
                    let events = self.context.on_app_activated(app, title, now);
                    self.emit(events);
                    if switched {
                        self.reset_focus();
                    }
                } else {
                    self.context.on_unobservable();
                    self.reset_focus();
                    self.unobservable = Some(match decision {
                        ObservationDecision::Paused => UnobservableReason::Paused,
                        ObservationDecision::Disabled => UnobservableReason::Disabled,
                        _ => UnobservableReason::Excluded,
                    });
                }
            }
            Observation::Unobservable(reason) => {
                self.context.on_unobservable();
                self.reset_focus();
                self.unobservable = Some(reason);
            }
            Observation::Focus(None) => {
                if self.focus.is_some() {
                    self.reset_focus();
                    self.context.on_focus(None, now);
                }
            }
            Observation::Focus(Some(input)) => self.on_focus(input),
            Observation::Clipboard { text, source_app } => {
                let events = self.context.on_clipboard(&text, source_app, now);
                self.emit(events);
            }
            Observation::ClipboardUnreadable => self.context.forget_clipboard(),
        }
    }

    fn reset_focus(&mut self) {
        self.cancel_pending();
        self.hide();
        self.focus = None;
        self.intent = None;
        self.completion_due = None;
        self.writing_due = None;
        self.hint_due = None;
    }

    fn cancel_pending(&mut self) {
        if let Some(p) = self.pending_completion.take() {
            p.cancel.cancel();
        }
        if let Some(p) = self.pending_grammar.take() {
            p.cancel.cancel();
        }
    }

    fn on_focus(&mut self, input: FocusedInput) {
        if self.unobservable.is_some() {
            // Never act on input from an app Mote may not observe.
            return;
        }
        if input.is_secure {
            self.reset_focus();
            return;
        }
        let now = Utc::now();
        let previous = self.focus.take();
        let new_element = previous.as_ref().map(|f| f.element_key) != Some(input.element_key);
        let text_changed = previous.as_ref().is_none_or(|f| {
            f.text_before_caret != input.text_before_caret
                || f.text_after_caret != input.text_after_caret
                || f.selected_text != input.selected_text
        });
        if new_element {
            self.cancel_pending();
            self.hide();
            let events = self.context.on_focus(Some(&input), now);
            self.emit(events);
        }
        self.focus = Some(input);
        if new_element || text_changed {
            self.classify();
        }
        if new_element {
            self.offer_context();
        } else if text_changed {
            self.on_text_changed();
        } else {
            self.reposition();
        }
        if text_changed || new_element {
            self.schedule();
        }
    }

    fn classify(&mut self) {
        let Some(focus) = &self.focus else { return };
        let full_text = format!("{}{}", focus.text_before_caret, focus.text_after_caret);
        self.language = detect(&focus.text_before_caret);
        let window = self.context.window(Utc::now());
        let user_override = None;
        let signals = IntentSignals {
            category: categorize(&focus.app, self.context.window_title()),
            role: Some(focus.role),
            is_multiline: focus.is_multiline,
            placeholder: focus.placeholder.as_deref(),
            label: focus.label.as_deref(),
            text: tail_chars(&full_text, 2_000),
            language: &self.language,
            clipboard_kind: window.clipboard.as_ref().map(|c| c.kind),
            previous_category: window.previous_category,
            user_override,
        };
        let deterministic = intent::classify(&signals);
        let element_key = focus.element_key;
        let assessment = match self.ai_intents.get(&element_key) {
            Some(ai) if deterministic.source != IntentSource::UserRule => ai.clone(),
            _ => deterministic,
        };
        let changed = self.intent.as_ref().is_none_or(|i| i.kind != assessment.kind);
        if changed {
            let event = ContextEventKind::IntentClassified {
                app: focus.app.name.clone(),
                kind: assessment.kind,
                confidence: assessment.confidence,
            };
            self.intent = Some(assessment.clone());
            self.record(event);
        } else {
            self.intent = Some(assessment.clone());
        }
        if self.settings.context.ai_classification
            && !self.ai_intents.contains_key(&element_key)
            && self.classification_pending.is_none_or(|(_, key)| key != element_key)
            && intent::needs_ai_classification(&assessment, &full_text)
            && self.deps.ai.automatic_requests_allowed()
        {
            self.request_classification();
        }
    }

    fn request_classification(&mut self) {
        let Some(focus) = self.focus.clone() else { return };
        let id = self.id();
        let element_key = focus.element_key;
        let app_name = focus.app.name.clone();
        let category = categorize(&focus.app, self.context.window_title());
        let role = focus.role;
        let placeholder = focus.placeholder.clone();
        let language = self.language.clone();
        let excerpt = tail_at_word_boundary(&focus.text_before_caret, CLASSIFICATION_EXCERPT_CHARS).to_string();
        let ai = self.deps.ai.clone();
        let tx = self.tx.clone();
        self.classification_pending = Some((id, element_key));
        tokio::spawn(async move {
            let cancel = CancellationToken::new();
            let prompt = ClassificationPrompt {
                app_name: &app_name,
                category,
                role: Some(role),
                placeholder: placeholder.as_deref(),
                language: &language,
                excerpt: &excerpt,
            };
            let result = ai.classify(&prompt, &cancel).await;
            let _ = tx.send(EngineInput::ClassificationReady { id, element_key, result }).await;
        });
    }

    fn on_classification(
        &mut self,
        id: u64,
        element_key: u64,
        result: Result<Option<IntentAssessment>, ProviderError>,
    ) {
        if self.classification_pending.is_some_and(|(pending, _)| pending == id) {
            self.classification_pending = None;
        }
        match result {
            Ok(Some(assessment)) => {
                if self.ai_intents.len() > 64 {
                    self.ai_intents.clear();
                }
                self.ai_intents.insert(element_key, assessment);
                if self.focus.as_ref().is_some_and(|f| f.element_key == element_key) {
                    self.classify();
                }
            }
            Ok(None) => {}
            Err(error) => self.on_provider_error(&error),
        }
    }

    fn on_text_changed(&mut self) {
        let Some(focus) = &self.focus else { return };
        let text_before = focus.text_before_caret.clone();
        match &mut self.active {
            Some(Active::Completion(set)) => match set.on_text(&text_before) {
                SuggestionUpdate::Keep(_) => {
                    self.show_active();
                    return;
                }
                SuggestionUpdate::Consumed => {
                    self.record(ContextEventKind::SuggestionAccepted { feature: Feature::InlineCompletion });
                    self.hide();
                }
                SuggestionUpdate::Invalid => self.hide(),
            },
            Some(_) => self.hide(),
            None => {}
        }
        if self.pending_completion.as_ref().is_some_and(|p| p.anchor != text_before && !p.alternative) {
            if let Some(p) = self.pending_completion.take() {
                p.cancel.cancel();
            }
        }
    }

    fn schedule(&mut self) {
        let Some(focus) = &self.focus else { return };
        let kind = self.intent.as_ref().map_or(IntentKind::Unknown, |i| i.kind);
        let now = Instant::now();
        if self.settings.completion.enabled && self.active.is_none() {
            self.completion_due = Some(now + Duration::from_millis(u64::from(self.settings.completion.debounce_ms)));
        }
        if self.settings.writing.enabled && writing_applies(kind) {
            let boundary = focus.text_before_caret.ends_with(|c: char| c.is_whitespace() || ".,!?;:".contains(c));
            self.writing_due = Some(now + if boundary { WRITING_DELAY_AFTER_BOUNDARY } else { WRITING_DELAY_MID_WORD });
        }
        if kind == IntentKind::Prompt
            && self.settings.prompts.enhancement_enabled
            && self.settings.prompts.show_hint
            && !self.hinted.contains(&focus.element_key)
        {
            self.hint_due = Some(now + HINT_DELAY);
        }
    }

    async fn on_timer(&mut self) {
        let now = Instant::now();
        if self.chip_expires.is_some_and(|t| t <= now) {
            self.chip_expires = None;
            if matches!(self.active, Some(Active::Hint | Active::Context(_))) {
                self.hide();
            }
        }
        if self.writing_due.is_some_and(|t| t <= now) {
            self.writing_due = None;
            self.try_writing();
        }
        if self.completion_due.is_some_and(|t| t <= now) {
            self.completion_due = None;
            self.try_complete(false);
        }
        if self.hint_due.is_some_and(|t| t <= now) {
            self.hint_due = None;
            self.try_hint();
        }
        self.publish_status();
    }

    fn current_kind(&self) -> IntentKind {
        self.intent.as_ref().map_or(IntentKind::Unknown, |i| i.kind)
    }

    fn try_complete(&mut self, alternative: bool) {
        let Some(focus) = self.focus.clone() else { return };
        let kind = self.current_kind();
        let key = anchor_key(&focus.app.id, kind, &focus.text_before_caret);
        let existing: Vec<String> = match &self.active {
            Some(Active::Completion(set)) if alternative => set.candidates().to_vec(),
            Some(_) => return,
            None if alternative => return,
            None => Vec::new(),
        };
        if !alternative {
            let since = self.last_completion_request.map(|t| t.elapsed());
            match should_complete(&focus, kind, &self.settings.completion, since, self.dismissed.contains(key)) {
                Ok(()) => {}
                Err(SkipReason::TooSoon) => {
                    // Try again once the minimum interval has passed.
                    let interval = Duration::from_millis(u64::from(self.settings.completion.min_interval_ms));
                    self.completion_due = self.last_completion_request.map(|t| t + interval);
                    return;
                }
                Err(_) => return,
            }
            if let Some(candidates) = self.cache.get(key) {
                if let Some(first) = candidates.first() {
                    let mut set = SuggestionSet::new(focus.text_before_caret.clone(), key, first.clone());
                    for c in candidates.iter().skip(1) {
                        set.push(c.clone());
                    }
                    self.show_suggestion(Active::Completion(set), Feature::InlineCompletion);
                }
                return;
            }
        }
        if self.pending_completion.is_some() || !self.deps.ai.automatic_requests_allowed() {
            return;
        }
        let id = self.id();
        let cancel = CancellationToken::new();
        let anchor = if alternative {
            match &self.active {
                Some(Active::Completion(set)) => set.anchor.clone(),
                _ => return,
            }
        } else {
            focus.text_before_caret.clone()
        };
        let context = tail_at_word_boundary(&anchor, COMPLETION_CONTEXT_CHARS).to_string();
        let language = self.language.clone();
        let subtype = self.intent.as_ref().and_then(|i| i.subtype);
        let category = categorize(&focus.app, self.context.window_title());
        let app_name = focus.app.name.clone();
        let max_words = self.settings.completion.max_words;
        let ai = self.deps.ai.clone();
        let tx = self.tx.clone();
        let task_cancel = cancel.clone();
        tokio::spawn(async move {
            let prompt = CompletionPrompt {
                text_before: &context,
                kind,
                subtype,
                language: &language,
                app_name: &app_name,
                category,
                max_words,
                avoid: &existing,
            };
            let result = ai.complete(&prompt, &task_cancel).await;
            let _ = tx.send(EngineInput::CompletionReady { id, result }).await;
        });
        self.last_completion_request = Some(Instant::now());
        self.pending_completion =
            Some(PendingCompletion { id, cancel, anchor, element_key: focus.element_key, key, alternative });
    }

    fn on_completion(&mut self, id: u64, result: Result<Option<String>, ProviderError>) {
        if self.pending_completion.as_ref().is_none_or(|p| p.id != id) {
            return;
        }
        let Some(pending) = self.pending_completion.take() else { return };
        match result {
            Ok(Some(text)) => {
                self.provider_problem = None;
                let Some(focus) = &self.focus else { return };
                if focus.element_key != pending.element_key {
                    return;
                }
                if pending.alternative {
                    if let Some(Active::Completion(set)) = &mut self.active {
                        if set.anchor == pending.anchor && set.push(text) {
                            set.select_next();
                            let all = set.candidates().to_vec();
                            self.cache.put(pending.key, all);
                            self.show_active();
                        }
                    }
                    return;
                }
                if focus.text_before_caret != pending.anchor || self.active.is_some() {
                    return;
                }
                self.cache.put(pending.key, vec![text.clone()]);
                let set = SuggestionSet::new(pending.anchor, pending.key, text);
                self.show_suggestion(Active::Completion(set), Feature::InlineCompletion);
            }
            Ok(None) => self.provider_problem = None,
            Err(error) => self.on_provider_error(&error),
        }
    }

    fn try_writing(&mut self) {
        let Some(focus) = self.focus.clone() else { return };
        let kind = self.current_kind();
        if !self.settings.writing.enabled || !writing_applies(kind) || self.active.is_some() {
            return;
        }
        if self.settings.writing.spelling {
            if let Some(correction) = spelling_correction(&focus.text_before_caret, &self.language, &self.ignored_words)
            {
                let key = fnv1a64(format!("{}\u{0}{}", focus.element_key, correction.original_tail).as_bytes());
                if !self.checked.contains(&key) {
                    self.checked.insert(key);
                    self.show_suggestion(Active::Correction(correction), Feature::WritingAssistance);
                    return;
                }
            }
        }
        let grammar_context = matches!(kind, IntentKind::Conversation | IntentKind::Note);
        if !self.settings.writing.ai_grammar || !grammar_context || self.pending_grammar.is_some() {
            return;
        }
        if !self.deps.ai.automatic_requests_allowed() {
            return;
        }
        let subtype = self.intent.as_ref().and_then(|i| i.subtype);
        let Some(sentence) = grammar_candidate(&focus.text_before_caret, &self.language, subtype, &self.checked) else {
            return;
        };
        self.checked.insert(fnv1a64(sentence.as_bytes()));
        let id = self.id();
        let cancel = CancellationToken::new();
        let ai = self.deps.ai.clone();
        let tx = self.tx.clone();
        let language = self.language.clone();
        let task_sentence = sentence.clone();
        let task_cancel = cancel.clone();
        tokio::spawn(async move {
            let result = ai.check_grammar(&task_sentence, &language, &task_cancel).await;
            let _ = tx.send(EngineInput::GrammarReady { id, result }).await;
        });
        self.pending_grammar = Some(PendingGrammar { id, cancel, sentence, element_key: focus.element_key });
    }

    fn on_grammar(&mut self, id: u64, result: Result<Option<String>, ProviderError>) {
        if self.pending_grammar.as_ref().is_none_or(|p| p.id != id) {
            return;
        }
        let Some(pending) = self.pending_grammar.take() else { return };
        match result {
            Ok(Some(corrected)) => {
                self.provider_problem = None;
                let Some(focus) = &self.focus else { return };
                if focus.element_key != pending.element_key
                    || !focus.text_before_caret.trim_end().ends_with(pending.sentence.as_str())
                    || self.active.is_some()
                {
                    return;
                }
                // Keep any whitespace typed after the sentence.
                let trailing = &focus.text_before_caret[focus.text_before_caret.trim_end().len()..];
                if let Some(mut correction) = correction_from_ai(&pending.sentence, &corrected) {
                    correction.original_tail.push_str(trailing);
                    correction.corrected_tail.push_str(trailing);
                    self.show_suggestion(Active::Correction(correction), Feature::WritingAssistance);
                }
            }
            Ok(None) => self.provider_problem = None,
            Err(error) => self.on_provider_error(&error),
        }
    }

    fn try_hint(&mut self) {
        let Some(focus) = &self.focus else { return };
        if self.current_kind() != IntentKind::Prompt
            || self.active.is_some()
            || self.pending_completion.is_some()
            || self.hinted.contains(&focus.element_key)
            || focus.text_before_caret.trim().chars().count() < MIN_HINT_CHARS
        {
            return;
        }
        self.hinted.insert(focus.element_key);
        self.chip_expires = Some(Instant::now() + CHIP_LIFETIME);
        self.show_suggestion(Active::Hint, Feature::PromptEnhancement);
    }

    fn offer_context(&mut self) {
        if !self.settings.context.contextual_suggestions || self.active.is_some() {
            return;
        }
        let Some(focus) = &self.focus else { return };
        let now = Utc::now();
        let text_chars = focus.text_before_caret.chars().count() + focus.text_after_caret.chars().count();
        let window = self.context.window(now);
        let Some(suggestion) = insights::suggest(&window, self.current_kind(), text_chars, now) else { return };
        if self.offered_clipboard == Some(suggestion.captured_at) {
            return;
        }
        self.offered_clipboard = Some(suggestion.captured_at);
        self.chip_expires = Some(Instant::now() + CHIP_LIFETIME);
        self.show_suggestion(Active::Context(suggestion), Feature::ContextAnalysis);
    }

    fn view(&self) -> Option<OverlayView> {
        let anchor = self.focus.as_ref().and_then(|f| f.caret_rect).filter(Rect::is_plausible);
        let space = self.deps.platform.coordinate_space();
        let palette = shortcut_label(&self.settings.keyboard.command_palette);
        let view = |kind, text: String, detail: Option<String>, index: usize, count: usize, hint: Option<String>| {
            OverlayView {
                kind,
                text,
                detail,
                index: u32::try_from(index).unwrap_or(0),
                count: u32::try_from(count).unwrap_or(0),
                anchor,
                coordinate_space: space,
                accept_hint: hint,
            }
        };
        Some(match self.active.as_ref()? {
            Active::Completion(set) => view(
                OverlayKind::Completion,
                set.current().to_string(),
                None,
                set.index(),
                set.len(),
                Some("Tab".into()),
            ),
            Active::Correction(correction) => {
                let detail = if correction.changes > 1 {
                    format!(
                        "{} → {} (+{} more)",
                        correction.display_from,
                        correction.display_to,
                        correction.changes - 1
                    )
                } else {
                    format!("{} → {}", correction.display_from, correction.display_to)
                };
                view(OverlayKind::Correction, correction.corrected_tail.clone(), Some(detail), 0, 1, Some("Tab".into()))
            }
            Active::Hint => view(OverlayKind::PromptHint, "Enhance prompt".into(), Some(palette), 0, 1, None),
            Active::Context(s) => {
                let actions = s.actions.iter().map(|a| a.display_name()).collect::<Vec<_>>().join(" · ");
                view(
                    OverlayKind::Context,
                    format!(
                        "Use copied {} from {}",
                        s.clipboard_kind.display_name().trim_start_matches("a ").trim_start_matches("an "),
                        s.source_app
                    ),
                    Some(format!("{palette} · {actions}")),
                    0,
                    1,
                    None,
                )
            }
        })
    }

    fn show_suggestion(&mut self, active: Active, feature: Feature) {
        self.active = Some(active);
        self.show_active();
        self.record(ContextEventKind::SuggestionShown { feature });
    }

    fn show_active(&self) {
        let Some(view) = self.view() else { return };
        self.deps.shell.show(&view);
        let accepts_keys = matches!(self.active, Some(Active::Completion(_) | Active::Correction(_)));
        self.deps.shell.set_suggestion_keys(accepts_keys);
    }

    fn reposition(&self) {
        if self.active.is_some() {
            self.show_active();
        }
    }

    fn hide(&mut self) {
        if self.active.take().is_some() {
            self.deps.shell.hide();
            self.deps.shell.set_suggestion_keys(false);
        }
        self.chip_expires = None;
    }

    async fn shortcut(&mut self, action: ShortcutAction) {
        match action {
            ShortcutAction::Accept => self.accept().await,
            ShortcutAction::Dismiss => self.dismiss().await,
            ShortcutAction::Next => self.cycle(true),
            ShortcutAction::Previous => self.cycle(false),
        }
    }

    async fn accept(&mut self) {
        let Some(active) = self.active.take() else {
            self.pass_through(Key::Tab).await;
            return;
        };
        self.deps.shell.hide();
        self.deps.shell.set_suggestion_keys(false);
        self.chip_expires = None;
        let (plan, feature, expected) = match active {
            Active::Completion(set) => {
                let expected = tail_chars(&set.expected_text(), 60).to_string();
                (EditPlan::Insert { text: set.current().to_string() }, Feature::InlineCompletion, Some(expected))
            }
            Active::Correction(correction) => (
                EditPlan::ReplaceBeforeCaret {
                    expected_tail: correction.original_tail.clone(),
                    replacement: correction.corrected_tail.clone(),
                },
                Feature::WritingAssistance,
                None,
            ),
            Active::Hint | Active::Context(_) => {
                self.pass_through(Key::Tab).await;
                return;
            }
        };
        let platform = self.deps.platform.clone();
        let own = self.deps.own_clipboard_writes.clone();
        let outcome = tokio::task::spawn_blocking(move || -> Result<(), PlatformError> {
            if let Some(expected) = expected {
                let current = platform.focused_input(ReadLimits::default())?.ok_or(PlatformError::NoFocusedElement)?;
                if !current.text_before_caret.ends_with(&expected) {
                    return Err(PlatformError::Failed("stale suggestion".into()));
                }
            }
            apply_edit(platform.as_ref(), &plan, own.as_ref())
        })
        .await;
        match outcome {
            Ok(Ok(())) => self.record(ContextEventKind::SuggestionAccepted { feature }),
            Ok(Err(PlatformError::Failed(_) | PlatformError::NoFocusedElement)) => {
                tracing::debug!("suggestion was stale when accepted; passing Tab through");
                self.pass_through(Key::Tab).await;
            }
            Ok(Err(error)) => tracing::warn!(error = %error, "could not apply suggestion"),
            Err(error) => tracing::warn!(error = %error, "edit task failed"),
        }
    }

    async fn dismiss(&mut self) {
        let Some(active) = self.active.take() else {
            self.pass_through(Key::Escape).await;
            return;
        };
        self.deps.shell.hide();
        self.deps.shell.set_suggestion_keys(false);
        self.chip_expires = None;
        let feature = match &active {
            Active::Completion(set) => {
                self.dismissed.insert(set.key);
                Feature::InlineCompletion
            }
            Active::Correction(correction) => {
                if correction.source == crate::assistance::CorrectionSource::LocalSpelling {
                    self.ignored_words.insert(correction.display_from.to_lowercase());
                }
                Feature::WritingAssistance
            }
            Active::Hint => Feature::PromptEnhancement,
            Active::Context(_) => Feature::ContextAnalysis,
        };
        self.record(ContextEventKind::SuggestionDismissed { feature });
    }

    fn cycle(&mut self, forward: bool) {
        let moved = match &mut self.active {
            Some(Active::Completion(set)) => {
                if forward {
                    set.select_next()
                } else {
                    set.select_previous()
                }
            }
            _ => return,
        };
        if moved {
            self.show_active();
        } else if forward
            && self.active.as_ref().is_some_and(|a| matches!(a, Active::Completion(s) if s.len() < MAX_ALTERNATIVES))
        {
            self.try_complete(true);
        }
    }

    /// Re-sends a key the user pressed while Mote held the shortcut but had
    /// nothing to apply, so their keystroke is never lost.
    async fn pass_through(&mut self, key: Key) {
        self.deps.shell.set_suggestion_keys(false);
        if self.last_tab_passthrough.is_some_and(|t| t.elapsed() < TAB_LOOP_GUARD) {
            return;
        }
        self.last_tab_passthrough = Some(Instant::now());
        let platform = self.deps.platform.clone();
        let _ = tokio::task::spawn_blocking(move || {
            std::thread::sleep(Duration::from_millis(40));
            platform.press_key(key, 1)
        })
        .await;
    }

    fn on_provider_error(&mut self, error: &ProviderError) {
        let state = match error {
            ProviderError::Cancelled => return,
            ProviderError::RateLimited { .. } => EngineState::RateLimited,
            ProviderError::Network(_) | ProviderError::Timeout => EngineState::Offline,
            ProviderError::Unauthorized | ProviderError::NotConfigured => EngineState::NeedsApiKey,
            ProviderError::CloudDisabled => EngineState::CloudDisabled,
            ProviderError::ModelUnavailable { .. }
            | ProviderError::Server { .. }
            | ProviderError::BadRequest { .. }
            | ProviderError::InvalidResponse(_) => EngineState::Active,
        };
        tracing::debug!(kind = error.kind(), "provider request failed");
        self.provider_problem = Some((state, error.user_message()));
    }

    fn compute_status(&self) -> EngineStatus {
        let routing = self.deps.ai.routing();
        let provider = self.deps.ai.provider();
        let (state, message) = if let Some(reason) = self.unobservable {
            let state = match reason {
                UnobservableReason::Disabled => EngineState::Disabled,
                UnobservableReason::Paused => EngineState::Paused,
                UnobservableReason::PermissionDenied => EngineState::NeedsPermission,
                UnobservableReason::SecureInput => EngineState::SecureInput,
                UnobservableReason::Excluded => EngineState::Excluded,
            };
            (state, None)
        } else if !routing.cloud_enabled {
            (EngineState::CloudDisabled, None)
        } else if !routing.configured {
            (EngineState::NeedsApiKey, Some(ProviderError::NotConfigured.user_message()))
        } else if provider.is_offline() {
            (EngineState::Offline, Some(ProviderError::Network(String::new()).user_message()))
        } else if let Some(until) = provider.rate_limited_until() {
            let wait = until.saturating_duration_since(Instant::now());
            (
                EngineState::RateLimited,
                Some(ProviderError::RateLimited { retry_after: Some(wait), snapshot: None }.user_message()),
            )
        } else if let Some((state, message)) = self.provider_problem.clone() {
            if matches!(state, EngineState::RateLimited | EngineState::Offline) {
                (if self.focus.is_some() { EngineState::Active } else { EngineState::Idle }, None)
            } else {
                (state, Some(message))
            }
        } else if self.focus.is_some() {
            (EngineState::Active, None)
        } else {
            (EngineState::Idle, None)
        };
        EngineStatus {
            state,
            message,
            app: self.context.active_app().map(|a| a.name.clone()),
            intent: self.intent.as_ref().map(|i| i.kind),
            subtype: self.intent.as_ref().and_then(|i| i.subtype),
            intent_confidence: self.intent.as_ref().map(|i| i.confidence),
            language: self.focus.as_ref().map(|_| self.language.label),
        }
    }

    fn publish_status(&mut self) {
        let status = self.compute_status();
        if self.last_status.as_ref() != Some(&status) {
            self.deps.shell.status(&status);
            self.last_status = Some(status);
        }
    }

    fn update_snapshot(&self) {
        let now = Utc::now();
        let snapshot = EngineSnapshot {
            app: self.context.active_app().cloned(),
            focus: self.focus.clone(),
            intent: self.intent.clone(),
            language: self.focus.as_ref().map(|_| self.language.clone()),
            clipboard: self.context.clipboard(now).cloned(),
            context_suggestion: match &self.active {
                Some(Active::Context(s)) => Some(s.clone()),
                _ => None,
            },
            status: self.last_status.clone().unwrap_or_default(),
        };
        *self.snapshot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
    }
}

/// Human-readable form of an accelerator, e.g. "⌘⇧Space" on macOS.
pub fn shortcut_label(accelerator: &str) -> String {
    let mac = cfg!(target_os = "macos");
    accelerator
        .split('+')
        .map(|part| match part.to_lowercase().as_str() {
            "commandorcontrol" | "cmdorctrl" => (if mac { "⌘" } else { "Ctrl+" }).to_string(),
            "command" | "cmd" | "super" | "meta" => (if mac { "⌘" } else { "Win+" }).to_string(),
            "control" | "ctrl" => (if mac { "⌃" } else { "Ctrl+" }).to_string(),
            "alt" | "option" => (if mac { "⌥" } else { "Alt+" }).to_string(),
            "shift" => (if mac { "⇧" } else { "Shift+" }).to_string(),
            "bracketright" => "]".to_string(),
            "bracketleft" => "[".to_string(),
            other => {
                let mut c = other.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
        })
        .collect()
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}

/// Maps a privacy decision to the reason reported to the engine.
pub fn unobservable_reason(decision: ObservationDecision) -> Option<UnobservableReason> {
    match decision {
        ObservationDecision::Allowed => None,
        ObservationDecision::Paused => Some(UnobservableReason::Paused),
        ObservationDecision::Disabled => Some(UnobservableReason::Disabled),
        ObservationDecision::Excluded(_) => Some(UnobservableReason::Excluded),
    }
}

#[cfg(test)]
mod tests;
