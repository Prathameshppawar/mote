//! IPC commands. Every input is validated here; secrets are accepted but never
//! returned; errors are user-facing [`CommandError`]s.

use std::sync::Arc;

use chrono::{Duration, Local, Timelike, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use mote_core::context::ContextEvent;
use mote_core::engine::{EngineInput, EngineStatus};
use mote_core::platform::{AppInfo, OsPlatform, PermissionState, PermissionStatus};
use mote_core::privacy::{always_excluded_descriptions, ExclusionKind, ExclusionRule};
use mote_core::providers::types::{HealthReport, ModelRole, RateLimitSnapshot};
use mote_core::providers::ModelProvider;
use mote_core::settings::Settings;
use mote_core::usage::dashboard::{build, DashboardInput, UsageDashboard};
use mote_core::usage::pricing::{ModelPricing, PricingCatalog};
use mote_providers::{ApiKey, GroqProvider};
use mote_storage::SuggestionStats;

use crate::diagnostics::{self, shorten_path, Diagnostics};
use crate::error::{CommandError, CommandResult};
use crate::palette::{
    self, PaletteApplyRequest, PaletteApplyResult, PaletteContext, PaletteRunRequest, PaletteRunResult,
};
use crate::secrets::GROQ_ACCOUNT;
use crate::settings_ops;
use crate::shell::DesktopShell;
use crate::state::AppState;
use crate::windows;

type AppStateRef<'a> = State<'a, Arc<AppState>>;

// ---- app -------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppInfoResponse {
    pub version: String,
    pub identifier: String,
    pub platform: OsPlatform,
    pub arch: String,
    pub data_dir: String,
    pub log_dir: String,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle, state: AppStateRef<'_>) -> AppInfoResponse {
    AppInfoResponse {
        version: env!("CARGO_PKG_VERSION").into(),
        identifier: app.config().identifier.clone(),
        platform: state.platform.os(),
        arch: std::env::consts::ARCH.into(),
        data_dir: shorten_path(&state.data_dir),
        log_dir: shorten_path(&state.log_dir),
    }
}

#[tauri::command]
pub fn get_engine_status(state: AppStateRef<'_>) -> EngineStatus {
    state.engine_status()
}

#[tauri::command]
pub fn get_permission_status(state: AppStateRef<'_>) -> PermissionStatus {
    state.platform.permission_status()
}

#[tauri::command]
pub async fn request_accessibility_permission(state: AppStateRef<'_>) -> CommandResult<PermissionState> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || platform.request_accessibility_permission())
        .await
        .map_err(|_| CommandError::new("platform", "Could not request permission."))
}

#[tauri::command]
pub fn open_accessibility_settings(state: AppStateRef<'_>) -> CommandResult<()> {
    Ok(state.platform.open_permission_settings()?)
}

#[tauri::command]
pub fn set_paused(app: AppHandle, state: AppStateRef<'_>, minutes: Option<u32>) -> CommandResult<Settings> {
    settings_ops::set_paused(&app, state.inner(), minutes)
}

#[tauri::command]
pub fn complete_onboarding(app: AppHandle, state: AppStateRef<'_>) -> CommandResult<Settings> {
    let mut settings = state.settings();
    settings.general.onboarding_completed = true;
    settings_ops::save(&app, state.inner(), settings)
}

#[tauri::command]
pub async fn get_diagnostics(state: AppStateRef<'_>) -> CommandResult<Diagnostics> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || diagnostics::collect(&state))
        .await
        .map_err(|_| CommandError::new("diagnostics", "Could not collect diagnostics."))
}

/// Copies a sanitized diagnostics report to the clipboard and returns it.
#[tauri::command]
pub async fn copy_diagnostics(state: AppStateRef<'_>) -> CommandResult<String> {
    let state = state.inner().clone();
    let text = tauri::async_runtime::spawn_blocking(move || {
        let report = diagnostics::to_text(&diagnostics::collect(&state));
        let _ = state.platform.set_clipboard_text(&report);
        report
    })
    .await
    .map_err(|_| CommandError::new("diagnostics", "Could not collect diagnostics."))?;
    Ok(text)
}

// ---- settings ----------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: AppStateRef<'_>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: AppStateRef<'_>, settings: Settings) -> CommandResult<Settings> {
    // The pause state is controlled by set_paused, not by the settings form.
    let mut settings = settings;
    settings.general.paused_until = state.settings().general.paused_until;
    settings_ops::save(&app, state.inner(), settings)
}

// ---- provider ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider: String,
    pub display_name: String,
    pub has_api_key: bool,
    /// Last four characters of the key, e.g. "…4Kcw".
    pub key_hint: Option<String>,
    pub base_url: String,
    pub health: Option<HealthReport>,
    pub limits: Option<RateLimitSnapshot>,
    pub unavailable_models: Vec<String>,
}

fn provider_status(state: &AppState) -> ProviderStatus {
    ProviderStatus {
        provider: GroqProvider::ID.into(),
        display_name: GroqProvider::DISPLAY_NAME.into(),
        has_api_key: state.has_api_key(),
        key_hint: state.key_hint(),
        base_url: state.settings().provider.groq.base_url,
        health: state.last_health(),
        limits: state.groq.usage_snapshot(),
        unavailable_models: state.resilient.unavailable_models(),
    }
}

fn required_models(settings: &Settings) -> Vec<String> {
    let m = &settings.provider.groq.models;
    let mut models = vec![
        m.completion.clone(),
        m.classification.clone(),
        m.writing.clone(),
        m.reasoning.clone(),
        m.fallback.clone(),
    ];
    models.sort();
    models.dedup();
    models
}

#[tauri::command]
pub fn get_provider_status(state: AppStateRef<'_>) -> ProviderStatus {
    provider_status(state.inner())
}

fn validate_key(key: &str) -> CommandResult<ApiKey> {
    let key = ApiKey::new(key);
    let len = key.expose().len();
    if !(16..=256).contains(&len) || !key.expose().chars().all(|c| c.is_ascii_graphic()) {
        return Err(CommandError::invalid("That doesn't look like an API key. Copy it again from the Groq console."));
    }
    Ok(key)
}

#[tauri::command]
pub async fn set_api_key(app: AppHandle, state: AppStateRef<'_>, key: String) -> CommandResult<ProviderStatus> {
    let key = validate_key(&key)?;
    state
        .secrets
        .set(GROQ_ACCOUNT, key.expose())
        .map_err(|e| CommandError::new("keychain", format!("Could not save the key: {e}.")))?;
    state.load_api_key();
    state.resilient.reset();
    state.clear_models_cache();
    refresh_engine_status(&state);
    let report = state.groq.health_check(&required_models(&state.settings())).await;
    state.set_last_health(report);
    let _ = app.emit("provider-changed", ());
    Ok(provider_status(state.inner()))
}

#[tauri::command]
pub fn clear_api_key(app: AppHandle, state: AppStateRef<'_>) -> CommandResult<ProviderStatus> {
    state
        .secrets
        .delete(GROQ_ACCOUNT)
        .map_err(|e| CommandError::new("keychain", format!("Could not remove the key: {e}.")))?;
    state.load_api_key();
    *state.last_health.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    state.clear_models_cache();
    refresh_engine_status(&state);
    let _ = app.emit("provider-changed", ());
    Ok(provider_status(state.inner()))
}

/// Re-applies the current settings in the engine, which drops a stale provider
/// problem (such as "needs API key") and republishes the status.
fn refresh_engine_status(state: &AppState) {
    let _ = state.engine.tx.try_send(EngineInput::Settings(Box::new(state.settings())));
}

/// Tests the saved key, or a candidate key without saving it.
#[tauri::command]
pub async fn test_connection(state: AppStateRef<'_>, key: Option<String>) -> CommandResult<HealthReport> {
    let settings = state.settings();
    let required = required_models(&settings);
    match key.filter(|k| !k.trim().is_empty()) {
        Some(candidate) => {
            let candidate = validate_key(&candidate)?;
            Ok(GroqProvider::verify_key(&settings.provider.groq.base_url, candidate, &required).await)
        }
        None => {
            let report = state.groq.health_check(&required).await;
            state.set_last_health(report.clone());
            Ok(report)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub owned_by: Option<String>,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub input_cost_per_million: Option<f64>,
    pub output_cost_per_million: Option<f64>,
    /// Roles this model is a sensible default for.
    pub recommended_for: Vec<ModelRole>,
}

#[tauri::command]
pub async fn list_models(state: AppStateRef<'_>, refresh: bool) -> CommandResult<Vec<ModelOption>> {
    let models = match state.models_cache().filter(|_| !refresh) {
        Some(models) => models,
        None => {
            let models = state.groq.list_models().await?;
            state.set_models_cache(models.clone());
            models
        }
    };
    let catalog = PricingCatalog::new(state.storage.list_pricing()?);
    let today = Local::now().date_naive();
    let defaults = mote_core::settings::ModelAssignments::default();
    Ok(models
        .into_iter()
        .filter(|m| m.supports_chat)
        .map(|m| {
            let price = catalog.price_at(GroqProvider::ID, &m.id, today);
            let mut recommended = Vec::new();
            if m.id == defaults.completion {
                recommended.extend([ModelRole::Completion, ModelRole::Classification, ModelRole::Writing]);
            }
            if m.id == defaults.reasoning {
                recommended.push(ModelRole::Reasoning);
            }
            ModelOption {
                input_cost_per_million: price.map(|p| p.input_cost_per_million),
                output_cost_per_million: price.map(|p| p.output_cost_per_million),
                id: m.id,
                owned_by: m.owned_by,
                context_window: m.context_window,
                max_output_tokens: m.max_output_tokens,
                recommended_for: recommended,
            }
        })
        .collect())
}

// ---- usage -------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageResponse {
    pub dashboard: UsageDashboard,
    #[cfg_attr(feature = "bindings", ts(type = "{ shown: number, accepted: number, dismissed: number }"))]
    pub suggestions_today: SuggestionStats,
    #[cfg_attr(feature = "bindings", ts(type = "{ shown: number, accepted: number, dismissed: number }"))]
    pub suggestions_30d: SuggestionStats,
    pub analytics_enabled: bool,
}

#[tauri::command]
pub async fn get_usage_dashboard(state: AppStateRef<'_>) -> CommandResult<UsageResponse> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> CommandResult<UsageResponse> {
        let now = Local::now();
        let today = now.date_naive();
        let buckets = state.storage.usage_buckets(today - Duration::days(31))?;
        let latencies = state.storage.latency_samples(Utc::now() - Duration::days(30), 20_000)?;
        let pricing = PricingCatalog::new(state.storage.list_pricing()?);
        let dashboard = build(DashboardInput {
            buckets: &buckets,
            latencies: &latencies,
            pricing: &pricing,
            today,
            current_hour: u8::try_from(now.hour()).unwrap_or(0),
            provider_limits: state.groq.usage_snapshot(),
            generated_at: Utc::now(),
        });
        let start_of_today = today
            .and_hms_opt(0, 0, 0)
            .and_then(|t| t.and_local_timezone(Local).earliest())
            .map_or_else(Utc::now, |t| t.with_timezone(&Utc));
        let suggestions_today = state.storage.suggestion_stats(start_of_today)?;
        let suggestions_30d = state.storage.suggestion_stats(Utc::now() - Duration::days(30))?;
        Ok(UsageResponse {
            dashboard,
            suggestions_today,
            suggestions_30d,
            analytics_enabled: state.settings().usage.analytics_enabled,
        })
    })
    .await
    .map_err(|_| CommandError::new("usage", "Could not load usage."))?
}

#[tauri::command]
pub fn list_pricing(state: AppStateRef<'_>) -> CommandResult<Vec<ModelPricing>> {
    Ok(state.storage.list_pricing()?)
}

#[tauri::command]
pub fn save_pricing(state: AppStateRef<'_>, pricing: ModelPricing) -> CommandResult<Vec<ModelPricing>> {
    state.storage.upsert_pricing(&pricing)?;
    Ok(state.storage.list_pricing()?)
}

#[tauri::command]
pub fn delete_pricing(state: AppStateRef<'_>, id: i64) -> CommandResult<Vec<ModelPricing>> {
    if !state.storage.delete_pricing(id)? {
        return Err(CommandError::invalid("Built-in prices can't be deleted; add your own price instead."));
    }
    Ok(state.storage.list_pricing()?)
}

#[tauri::command]
pub fn reset_pricing(state: AppStateRef<'_>) -> CommandResult<Vec<ModelPricing>> {
    state.storage.reset_pricing()?;
    Ok(state.storage.list_pricing()?)
}

#[tauri::command]
pub fn clear_usage_history(app: AppHandle, state: AppStateRef<'_>) -> CommandResult<u64> {
    let removed = state.storage.clear_usage()?;
    let _ = app.emit("usage-updated", ());
    Ok(removed)
}

// ---- privacy -----------------------------------------------------------------

#[tauri::command]
pub fn list_exclusions(state: AppStateRef<'_>) -> CommandResult<Vec<ExclusionRule>> {
    Ok(state.storage.list_exclusions()?)
}

#[tauri::command]
pub fn add_exclusion(
    state: AppStateRef<'_>,
    kind: ExclusionKind,
    pattern: String,
    display_name: String,
) -> CommandResult<Vec<ExclusionRule>> {
    state.storage.add_exclusion(kind, &pattern, &display_name)?;
    state.refresh_policy(&state.settings());
    Ok(state.storage.list_exclusions()?)
}

#[tauri::command]
pub fn remove_exclusion(state: AppStateRef<'_>, id: i64) -> CommandResult<Vec<ExclusionRule>> {
    state.storage.remove_exclusion(id)?;
    state.refresh_policy(&state.settings());
    Ok(state.storage.list_exclusions()?)
}

#[tauri::command]
pub async fn list_running_apps(state: AppStateRef<'_>) -> CommandResult<Vec<AppInfo>> {
    let platform = state.platform.clone();
    tauri::async_runtime::spawn_blocking(move || {
        platform
            .running_applications()
            .into_iter()
            .filter(|a| mote_core::intent::apps::categorize(a, None) != mote_core::intent::apps::AppCategory::Mote)
            .collect()
    })
    .await
    .map_err(|_| CommandError::new("platform", "Could not list applications."))
}

#[tauri::command]
pub fn get_always_excluded() -> Vec<String> {
    always_excluded_descriptions().into_iter().map(str::to_string).collect()
}

#[tauri::command]
pub fn get_recent_activity(state: AppStateRef<'_>, limit: u32) -> CommandResult<Vec<ContextEvent>> {
    Ok(state.storage.recent_context_events(limit.clamp(1, 500) as usize)?)
}

#[tauri::command]
pub fn clear_context(state: AppStateRef<'_>) -> CommandResult<()> {
    state.storage.clear_context()?;
    crate::palette::clear_session(&state);
    let _ = state.engine.tx.try_send(EngineInput::ClearContext);
    Ok(())
}

/// Deletes all local data, including the API key, and restarts onboarding.
#[tauri::command]
pub fn reset_local_data(app: AppHandle, state: AppStateRef<'_>) -> CommandResult<Settings> {
    state.storage.reset_all()?;
    crate::palette::clear_session(&state);
    state
        .secrets
        .delete(GROQ_ACCOUNT)
        .map_err(|e| CommandError::new("keychain", format!("Could not remove the key: {e}.")))?;
    state.load_api_key();
    state.resilient.reset();
    state.clear_models_cache();
    *state.last_health.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let _ = state.engine.tx.try_send(EngineInput::ClearContext);
    let settings = Settings::default();
    let saved = settings_ops::save(&app, state.inner(), settings)?;
    let _ = app.emit("usage-updated", ());
    Ok(saved)
}

// ---- palette & overlay -----------------------------------------------------------

#[tauri::command]
pub fn palette_context(state: AppStateRef<'_>) -> PaletteContext {
    palette::context(state.inner())
}

#[tauri::command]
pub async fn palette_run(state: AppStateRef<'_>, request: PaletteRunRequest) -> CommandResult<PaletteRunResult> {
    palette::run(state.inner().clone(), request).await
}

#[tauri::command]
pub fn palette_cancel(state: AppStateRef<'_>) {
    palette::cancel(state.inner());
}

#[tauri::command]
pub async fn palette_apply(
    app: AppHandle,
    state: AppStateRef<'_>,
    request: PaletteApplyRequest,
) -> CommandResult<PaletteApplyResult> {
    palette::apply(&app, state.inner().clone(), request).await
}

#[tauri::command]
pub fn palette_close(app: AppHandle, state: AppStateRef<'_>, reactivate: bool) {
    palette::close(&app, state.inner(), reactivate);
}

#[tauri::command]
pub fn palette_open_main(app: AppHandle, state: AppStateRef<'_>, section: String) -> CommandResult<()> {
    palette::close(&app, state.inner(), false);
    windows::show_main(&app, Some(section.as_str())).map_err(|_| CommandError::new("window", "Could not open Mote."))
}

#[tauri::command]
pub fn overlay_ready(shell: State<'_, Arc<DesktopShell>>, seq: u64, width: f64, height: f64) {
    if width.is_finite() && height.is_finite() {
        shell.on_overlay_ready(seq, width, height);
    }
}

// ---- updates ----------------------------------------------------------------

#[tauri::command]
pub fn get_update_status(
    app: AppHandle,
    updates: State<'_, Arc<crate::updates::Updates>>,
) -> crate::updates::UpdateStatus {
    updates.status(&app)
}

#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    updates: State<'_, Arc<crate::updates::Updates>>,
) -> CommandResult<crate::updates::UpdateStatus> {
    Ok(updates.inner().clone().check(&app).await)
}

#[tauri::command]
pub fn install_update(app: AppHandle, updates: State<'_, Arc<crate::updates::Updates>>) -> CommandResult<()> {
    updates.install_and_restart(&app)
}
