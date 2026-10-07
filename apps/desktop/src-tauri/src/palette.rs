//! The command palette: keyboard-first actions on the current selection, the
//! focused field or the clipboard.
//!
//! Context is captured *before* the palette takes focus. Results are applied
//! back into the original application only if the same field still has focus;
//! otherwise they are copied to the clipboard instead.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

use mote_core::assistance::{apply_edit, EditPlan};
use mote_core::context::clipboard::ClipboardKind;
use mote_core::context::insights::{ContextAction, ContextSuggestion};
use mote_core::context::manager::ClipboardSnapshot;
use mote_core::intent::{IntentAssessment, IntentKind, IntentSubtype};
use mote_core::language::{detect, LanguageLabel, LanguageProfile};
use mote_core::platform::{AppInfo, FocusedInput, PermissionState, ReadLimits};
use mote_core::prompts::{TransformAction, TransformPrompt};
use mote_core::text::{head_chars, utf16_len};

use crate::error::{CommandError, CommandResult};
use crate::shell::DesktopShell;
use crate::state::AppState;
use crate::windows;

/// Field text read for palette actions (much larger than the observer's, so
/// actions can work on a whole prompt or message). Documented in the privacy model.
const PALETTE_LIMITS: ReadLimits = ReadLimits { before_caret: 20_000, after_caret: 20_000, selection: 20_000 };
/// A captured session is discarded after this long, even if the palette was
/// never closed.
const SESSION_TTL: Duration = Duration::from_secs(10 * 60);
const PREVIEW_CHARS: usize = 280;
/// Longest text the palette will apply.
const MAX_APPLY_CHARS: usize = 100_000;

/// What the palette knows about the user's context. In memory only.
#[derive(Debug, Clone)]
pub struct PaletteSession {
    pub captured_at: Instant,
    pub app: Option<AppInfo>,
    pub focus: Option<FocusedInput>,
    pub field_truncated: bool,
    pub intent: Option<IntentAssessment>,
    pub language: Option<LanguageProfile>,
    pub clipboard: Option<ClipboardSnapshot>,
    pub context_suggestion: Option<ContextSuggestion>,
    pub source: Option<PaletteSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PaletteSource {
    Selection,
    Field,
    Clipboard,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TextPreview {
    pub text: String,
    pub chars: u32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ClipboardPreview {
    pub text: String,
    pub chars: u32,
    pub source_app: Option<String>,
    pub kind: ClipboardKind,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PaletteContext {
    pub app_name: Option<String>,
    pub intent: Option<IntentKind>,
    pub subtype: Option<IntentSubtype>,
    pub language: Option<LanguageLabel>,
    pub language_name: Option<String>,
    pub selection: Option<TextPreview>,
    pub field: Option<TextPreview>,
    pub field_truncated: bool,
    pub clipboard: Option<ClipboardPreview>,
    pub context_actions: Vec<ContextAction>,
    pub has_api_key: bool,
    pub cloud_enabled: bool,
    pub can_insert: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PaletteRunRequest {
    pub action: TransformAction,
    pub source: PaletteSource,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PaletteRunResult {
    pub text: String,
    pub source: PaletteSource,
    /// Whether the result can replace the source in place.
    pub can_replace: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ApplyMode {
    Replace,
    Insert,
    Copy,
}

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PaletteApplyRequest {
    pub text: String,
    pub mode: ApplyMode,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PaletteApplyResult {
    pub applied: bool,
    pub copied: bool,
    pub message: Option<String>,
}

fn preview(text: &str) -> TextPreview {
    TextPreview {
        text: head_chars(text, PREVIEW_CHARS).to_string(),
        chars: u32::try_from(text.chars().count()).unwrap_or(u32::MAX),
    }
}

/// The captured session, unless it has expired (then it is dropped).
fn current_session(state: &AppState) -> Option<PaletteSession> {
    let mut session = state.palette();
    if session.as_ref().is_some_and(|s| s.captured_at.elapsed() > SESSION_TTL) {
        *session = None;
    }
    session.clone()
}

/// Forgets captured text and clipboard content (close, apply, pause, clear, reset).
pub fn clear_session(state: &AppState) {
    *state.palette() = None;
}

fn field_text(focus: &FocusedInput) -> String {
    format!("{}{}{}", focus.text_before_caret, focus.selected_text.as_deref().unwrap_or(""), focus.text_after_caret)
}

/// Captures context (off the main thread) and shows the palette.
pub fn open(app: &AppHandle) {
    let Some(state) = app.try_state::<Arc<AppState>>() else { return };
    let state = state.inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let capture_state = state.clone();
        let session = tauri::async_runtime::spawn_blocking(move || capture(&capture_state)).await.ok();
        *state.palette() = session;
        if let Err(error) = windows::show_palette(&app) {
            tracing::warn!(%error, "could not show the command palette");
        }
    });
}

/// Reads the user's context, honouring the privacy policy exactly as the
/// observer does: nothing is read (not even the window title) during Secure
/// Input or without permission, and the focused field must belong to the
/// application the policy was evaluated for.
fn capture(state: &AppState) -> PaletteSession {
    let empty = PaletteSession {
        captured_at: Instant::now(),
        app: None,
        focus: None,
        field_truncated: false,
        intent: None,
        language: None,
        clipboard: None,
        context_suggestion: None,
        source: None,
    };
    let permissions = state.platform.permission_status();
    if permissions.secure_input_active
        || !matches!(permissions.accessibility, PermissionState::Granted | PermissionState::NotRequired)
    {
        return empty;
    }
    let snapshot = state.engine.snapshot();
    let policy = state.policy.borrow().clone();
    let now = Utc::now();
    let Some(app) = state.platform.active_application() else { return empty };
    let title = state.platform.active_window().and_then(|w| w.title);
    let decision = policy.evaluate(&app, title.as_deref(), now);
    if !decision.is_allowed() {
        return empty;
    }
    let mut focus = None;
    let mut field_truncated = false;
    if policy.may_read_text(decision) {
        if let Ok(Some(input)) = state.platform.focused_input(PALETTE_LIMITS) {
            if !input.is_secure && input.app.id == app.id {
                let read_units = utf16_len(&field_text(&input));
                field_truncated = input.total_length.is_some_and(|total| total > read_units);
                focus = Some(input);
            }
        }
    }
    let language = focus.as_ref().map(|f| detect(&field_text(f)));
    let clipboard_allowed = policy.may_read_clipboard(decision);
    PaletteSession {
        app: Some(app),
        intent: if focus.is_some() { snapshot.intent } else { None },
        focus,
        field_truncated,
        language,
        clipboard: snapshot.clipboard.filter(|_| clipboard_allowed),
        context_suggestion: snapshot.context_suggestion.filter(|_| clipboard_allowed),
        ..empty
    }
}

pub fn context(state: &AppState) -> PaletteContext {
    let session = current_session(state);
    let settings = state.settings();
    let Some(session) = session else {
        return PaletteContext {
            app_name: None,
            intent: None,
            subtype: None,
            language: None,
            language_name: None,
            selection: None,
            field: None,
            field_truncated: false,
            clipboard: None,
            context_actions: Vec::new(),
            has_api_key: state.has_api_key(),
            cloud_enabled: settings.privacy.cloud_ai_enabled,
            can_insert: false,
        };
    };
    let selection = session.focus.as_ref().and_then(|f| f.selected_text.clone()).filter(|s| !s.trim().is_empty());
    let field = session.focus.as_ref().map(field_text).filter(|t| !t.trim().is_empty());
    PaletteContext {
        app_name: session.app.as_ref().map(|a| a.name.clone()),
        intent: session.intent.as_ref().map(|i| i.kind),
        subtype: session.intent.as_ref().and_then(|i| i.subtype),
        language: session.language.as_ref().map(|l| l.label),
        language_name: session.language.as_ref().map(|l| l.label.display_name().to_string()),
        selection: selection.as_deref().map(preview),
        field: field.as_deref().map(preview),
        field_truncated: session.field_truncated,
        clipboard: session.clipboard.as_ref().map(|c| ClipboardPreview {
            text: head_chars(&c.text, PREVIEW_CHARS).to_string(),
            chars: u32::try_from(c.char_count).unwrap_or(u32::MAX),
            source_app: c.source_app.as_ref().map(|a| a.name.clone()),
            kind: c.kind,
        }),
        context_actions: session.context_suggestion.map(|s| s.actions).unwrap_or_default(),
        has_api_key: state.has_api_key(),
        cloud_enabled: settings.privacy.cloud_ai_enabled,
        can_insert: session.app.is_some() && session.focus.is_some(),
    }
}

/// Runs a transformation on the chosen source.
pub async fn run(state: Arc<AppState>, request: PaletteRunRequest) -> CommandResult<PaletteRunResult> {
    request.action.validate().map_err(CommandError::invalid)?;
    let session = current_session(&state).ok_or_else(|| CommandError::invalid("Open the palette again."))?;
    let focus = session.focus.as_ref();
    let clipboard = session.clipboard.as_ref();
    let is_context_action = matches!(request.action, TransformAction::UseContext { .. });
    let text = match (request.source, is_context_action) {
        (_, true) => focus.map(field_text).unwrap_or_default(),
        (PaletteSource::Selection, false) => focus.and_then(|f| f.selected_text.clone()).unwrap_or_default(),
        (PaletteSource::Field, false) => focus.map(field_text).unwrap_or_default(),
        (PaletteSource::Clipboard, false) => clipboard.map(|c| c.text.clone()).unwrap_or_default(),
    };
    let clip = if is_context_action {
        let c = clipboard.ok_or_else(|| CommandError::invalid("There is no recently copied content to use."))?;
        Some((c.source_app.as_ref().map_or_else(|| "another app".to_string(), |a| a.name.clone()), c.text.clone()))
    } else {
        None
    };
    if text.trim().is_empty() && clip.is_none() {
        return Err(CommandError::invalid("There is no text to work with. Select text or type in a field first."));
    }
    let language = detect(if text.trim().is_empty() { clip.as_ref().map_or("", |c| c.1.as_str()) } else { &text });
    let cancel = CancellationToken::new();
    state.set_palette_cancel(Some(cancel.clone()));
    let prompt = TransformPrompt {
        action: &request.action,
        text: &text,
        language: &language,
        kind: session.intent.as_ref().map(|i| i.kind),
        subtype: session.intent.as_ref().and_then(|i| i.subtype),
        clipboard: clip.as_ref().map(|(s, c)| (s.as_str(), c.as_str())),
    };
    let result = state.ai.transform(&prompt, &cancel).await;
    state.set_palette_cancel(None);
    let output = result?;
    let source = if is_context_action { PaletteSource::Field } else { request.source };
    if let Some(session) = state.palette().as_mut() {
        session.source = Some(source);
    }
    let can_replace = match source {
        PaletteSource::Selection => true,
        PaletteSource::Field => !session.field_truncated && !is_context_action,
        PaletteSource::Clipboard => false,
    } && session.app.is_some();
    Ok(PaletteRunResult { text: output, source, can_replace })
}

pub fn cancel(state: &AppState) {
    state.set_palette_cancel(None);
}

/// Applies a result: replace or insert into the original field, or copy.
pub async fn apply(
    app: &AppHandle,
    state: Arc<AppState>,
    request: PaletteApplyRequest,
) -> CommandResult<PaletteApplyResult> {
    if request.text.chars().count() > MAX_APPLY_CHARS {
        return Err(CommandError::invalid("The text is too long to insert."));
    }
    // Taken, not cloned: captured text is forgotten once the result is applied.
    let session = state.palette().take().filter(|s| s.captured_at.elapsed() <= SESSION_TTL);
    windows::hide_palette(app);
    let shell = app.try_state::<Arc<DesktopShell>>().map(|s| s.inner().clone());
    let copy = |state: &AppState, notice: &str| -> CommandResult<PaletteApplyResult> {
        state.platform.set_clipboard_text(&request.text)?;
        if let Some(shell) = &shell {
            shell.notice(notice);
        }
        Ok(PaletteApplyResult { applied: false, copied: true, message: Some(notice.to_string()) })
    };
    let target = session.as_ref().and_then(|s| Some((s.app.clone()?, s.focus.clone()?, s.source, s.field_truncated)));
    if request.mode == ApplyMode::Copy {
        if let Some((app_info, ..)) = &target {
            let _ = state.platform.activate_application(app_info);
        }
        return copy(&state, "Copied to clipboard");
    }
    let Some((app_info, focus, source, truncated)) = target else {
        return copy(&state, "No text field to insert into. Copied to clipboard");
    };
    let plan = match (request.mode, source) {
        (ApplyMode::Replace, Some(PaletteSource::Selection)) => {
            EditPlan::ReplaceSelection { text: request.text.clone() }
        }
        (ApplyMode::Replace, Some(PaletteSource::Field)) if !truncated => {
            EditPlan::ReplaceAll { text: request.text.clone() }
        }
        _ => EditPlan::Insert { text: request.text.clone() },
    };
    let platform = state.platform.clone();
    let own = state.own_clipboard.clone();
    let expected_key = focus.element_key;
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        platform.activate_application(&app_info)?;
        std::thread::sleep(Duration::from_millis(220));
        // Read the field only if the captured (allowed) application is in front.
        if platform.active_application().map(|a| a.id) != Some(app_info.id.clone()) {
            return Err(mote_core::platform::PlatformError::Failed("another application is in front".into()));
        }
        let current = platform.focused_input(ReadLimits::default())?;
        if current.as_ref().map(|c| c.element_key) != Some(expected_key) {
            return Err(mote_core::platform::PlatformError::Failed("focus moved to another field".into()));
        }
        apply_edit(platform.as_ref(), &plan, own.as_ref())
    })
    .await
    .map_err(|_| CommandError::new("platform", "Mote could not apply the text."))?;
    match outcome {
        Ok(()) => Ok(PaletteApplyResult { applied: true, copied: false, message: None }),
        Err(error) => {
            tracing::info!(error = %error, "could not apply palette result; copying instead");
            copy(&state, "Couldn't insert here. Copied to clipboard")
        }
    }
}

/// Hides the palette, optionally returning focus to the original app, and
/// forgets the captured context.
pub fn close(app: &AppHandle, state: &AppState, reactivate: bool) {
    windows::hide_palette(app);
    state.set_palette_cancel(None);
    let session = state.palette().take();
    if reactivate {
        if let Some(app_info) = session.and_then(|s| s.app) {
            let platform = state.platform.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let _ = platform.activate_application(&app_info);
            });
        }
    }
}
