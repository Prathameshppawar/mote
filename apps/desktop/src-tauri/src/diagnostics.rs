//! Diagnostics: a structured health report and a sanitized text copy.
//!
//! Diagnostics never include the API key, typed text, clipboard content,
//! prompts, generated text, window titles or application names; paths are
//! shortened so they do not reveal the user's account name.

use chrono::{Duration, Utc};
use serde::Serialize;

use mote_core::engine::EngineStatus;
use mote_core::platform::{PermissionStatus, PlatformAdapter};
use mote_core::privacy::redact::redact_secrets;
use mote_core::providers::resilient::RequestOutcome;
use mote_core::providers::types::{Feature, HealthReport, RateLimitSnapshot};
use mote_core::settings::ModelAssignments;
use mote_storage::StorageStats;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "bindings", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub provider: String,
    pub base_url: String,
    pub has_api_key: bool,
    pub models: ModelAssignments,
    pub unavailable_models: Vec<String>,
    pub provider_health: Option<HealthReport>,
    pub provider_limits: Option<RateLimitSnapshot>,
    pub permissions: PermissionStatus,
    pub clipboard_observation: bool,
    pub text_observation: bool,
    pub cloud_ai_enabled: bool,
    pub engine: EngineStatus,
    #[cfg_attr(
        feature = "bindings",
        ts(
            type = "{ schemaVersion: number, usageEvents: number, contextEvents: number, contextSessions: number, exclusions: number, pricingRows: number, sizeBytes: number | null } | null"
        )
    )]
    pub database: Option<StorageStats>,
    pub database_ok: bool,
    pub database_path: String,
    pub log_dir: String,
    pub avg_completion_latency_ms: Option<f64>,
    pub completion_requests_24h: u32,
    pub last_request: Option<RequestOutcome>,
}

/// Replaces the home directory with `~`.
pub fn shorten_path(path: &std::path::Path) -> String {
    let display = path.display().to_string();
    match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(home) => {
            let home = home.to_string_lossy().to_string();
            if !home.is_empty() && display.starts_with(&home) {
                format!("~{}", &display[home.len()..])
            } else {
                display
            }
        }
        None => display,
    }
}

fn os_description(platform: &dyn PlatformAdapter) -> String {
    let family = match platform.os() {
        mote_core::platform::OsPlatform::Macos => "macOS",
        mote_core::platform::OsPlatform::Windows => "Windows",
        mote_core::platform::OsPlatform::Other => std::env::consts::OS,
    };
    match os_version() {
        Some(version) => format!("{family} {version}"),
        None => family.to_string(),
    }
}

#[cfg(target_os = "macos")]
fn os_version() -> Option<String> {
    let output = std::process::Command::new("sw_vers").arg("-productVersion").output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string()).filter(|v| !v.is_empty())
}

#[cfg(not(target_os = "macos"))]
fn os_version() -> Option<String> {
    None
}

pub fn collect(state: &AppState) -> Diagnostics {
    let settings = state.settings();
    let since = Utc::now() - Duration::hours(24);
    let completion: Vec<u32> = state
        .storage
        .latency_samples(since, 5_000)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.feature == Feature::InlineCompletion)
        .map(|s| s.latency_ms)
        .collect();
    let avg = (!completion.is_empty())
        .then(|| completion.iter().map(|v| f64::from(*v)).sum::<f64>() / completion.len() as f64);
    let base_url = settings.provider.groq.base_url.clone();
    let mut engine = state.engine_status();
    engine.app = None; // the active application name is not needed for diagnostics
    Diagnostics {
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: os_description(state.platform.as_ref()),
        arch: std::env::consts::ARCH.to_string(),
        provider: "Groq".into(),
        base_url,
        has_api_key: state.has_api_key(),
        models: settings.provider.groq.models.clone(),
        unavailable_models: state.resilient.unavailable_models(),
        provider_health: state.last_health(),
        provider_limits: mote_core::providers::ModelProvider::usage_snapshot(state.groq.as_ref()),
        permissions: state.platform.permission_status(),
        clipboard_observation: settings.privacy.observe_clipboard,
        text_observation: settings.privacy.observe_text,
        cloud_ai_enabled: settings.privacy.cloud_ai_enabled,
        engine,
        database: state.storage.stats().ok(),
        database_ok: state.storage.integrity_ok(),
        database_path: state.storage.path().map(shorten_path).unwrap_or_else(|| "in-memory".into()),
        log_dir: shorten_path(&state.log_dir),
        avg_completion_latency_ms: avg,
        completion_requests_24h: u32::try_from(completion.len()).unwrap_or(u32::MAX),
        last_request: state.resilient.last_outcome(),
    }
}

/// A plain-text report safe to paste into a bug report.
pub fn to_text(d: &Diagnostics) -> String {
    let health = match &d.provider_health {
        Some(h) if h.ok => format!("ok ({} models, {} ms)", h.models_available, h.latency_ms.unwrap_or(0)),
        Some(h) => format!("problem: {}", h.message.clone().unwrap_or_default()),
        None => "not checked".into(),
    };
    let limits = d.provider_limits.as_ref().map_or_else(
        || "unknown".to_string(),
        |l| {
            format!(
                "requests {}/{} per day, tokens {}/{} per minute",
                l.requests_remaining.map_or("?".into(), |v| v.to_string()),
                l.requests_limit.map_or("?".into(), |v| v.to_string()),
                l.tokens_remaining.map_or("?".into(), |v| v.to_string()),
                l.tokens_limit.map_or("?".into(), |v| v.to_string()),
            )
        },
    );
    let last = d.last_request.as_ref().map_or_else(
        || "none".to_string(),
        |r| {
            format!(
                "{:?} via {} in {} ms ({}){}",
                r.status,
                r.model,
                r.latency_ms,
                r.feature.as_str(),
                r.error_kind.as_ref().map(|k| format!(", {k}")).unwrap_or_default()
            )
        },
    );
    let db = d.database.as_ref().map_or_else(
        || "unavailable".to_string(),
        |s| {
            format!(
                "schema v{}, {} usage events, {} context events, {} KB, integrity {}",
                s.schema_version,
                s.usage_events,
                s.context_events,
                s.size_bytes.unwrap_or(0) / 1024,
                if d.database_ok { "ok" } else { "FAILED" }
            )
        },
    );
    let text = format!(
        "Mote Diagnostics\n\
         ================\n\
         Version:               {}\n\
         OS:                    {} ({})\n\
         Provider:              {} ({})\n\
         API key configured:    {}\n\
         Models:                completion={}, classification={}, writing={}, reasoning={}, fallback={}\n\
         Unavailable models:    {}\n\
         Provider health:       {}\n\
         Provider limits:       {}\n\
         Accessibility:         {:?}\n\
         Secure input active:   {}\n\
         Text observation:      {}\n\
         Clipboard observation: {}\n\
         Cloud AI:              {}\n\
         Context engine:        {:?}{}\n\
         Database:              {}\n\
         Database path:         {}\n\
         Logs:                  {}\n\
         Avg completion latency (24h): {}\n\
         Last request:          {}\n",
        d.version,
        d.os,
        d.arch,
        d.provider,
        d.base_url,
        if d.has_api_key { "yes" } else { "no" },
        d.models.completion,
        d.models.classification,
        d.models.writing,
        d.models.reasoning,
        d.models.fallback,
        if d.unavailable_models.is_empty() { "none".to_string() } else { d.unavailable_models.join(", ") },
        health,
        limits,
        d.permissions.accessibility,
        d.permissions.secure_input_active,
        d.text_observation,
        d.clipboard_observation,
        d.cloud_ai_enabled,
        d.engine.state,
        d.engine.message.as_ref().map(|m| format!(" ({m})")).unwrap_or_default(),
        db,
        d.database_path,
        d.log_dir,
        d.avg_completion_latency_ms
            .map_or("n/a".to_string(), |v| format!("{v:.0} ms over {} requests", d.completion_requests_24h)),
        last,
    );
    redact_secrets(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_hide_the_home_directory() {
        if let Some(home) = std::env::var_os("HOME") {
            let path = std::path::Path::new(&home).join("Library/Application Support/mote/mote.db");
            let short = shorten_path(&path);
            assert!(short.starts_with("~/"), "{short}");
            assert!(!short.contains(&*home.to_string_lossy()));
        }
    }
}
