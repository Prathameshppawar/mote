//! # mote-storage
//!
//! Local SQLite storage. Everything Mote persists lives in one database file in
//! the user's application-data directory:
//!
//! * `settings`: the settings document (no secrets; the API key lives in the OS keychain)
//! * `usage_events`: one metadata row per model request (no prompts, no outputs)
//! * `model_pricing`: built-in and user-edited prices with effective dates
//! * `excluded_apps`: user exclusion rules
//! * `context_events` / `context_sessions`: activity metadata, pruned by the
//!   user's retention setting
//! * `user_preferences`, `provider_config`: small key-value data

mod migrations;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use mote_core::context::{ContextEvent, ContextEventKind};
use mote_core::privacy::{ExclusionKind, ExclusionRule};
use mote_core::providers::types::{Feature, RequestType};
use mote_core::settings::Settings;
use mote_core::usage::dashboard::{LatencySample, UsageBucket};
use mote_core::usage::pricing::{builtin_pricing, ModelPricing, PricingSource};
use mote_core::usage::{UsageEvent, UsageStatus};

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("could not create the data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid input: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

const SETTINGS_KEY: &str = "app";

/// Row counts and file information, for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStats {
    pub schema_version: u32,
    pub usage_events: u64,
    pub context_events: u64,
    pub context_sessions: u64,
    pub exclusions: u64,
    pub pricing_rows: u64,
    pub size_bytes: Option<u64>,
}

/// Suggestion outcomes in a period (from session bookkeeping).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionStats {
    pub shown: u64,
    pub accepted: u64,
    pub dismissed: u64,
}

/// Thread-safe handle to the database.
#[derive(Clone)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
    path: Option<PathBuf>,
    current_session: Arc<Mutex<Option<i64>>>,
}

fn ms(ts: DateTime<Utc>) -> i64 {
    ts.timestamp_millis()
}

fn from_ms(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms).single().unwrap_or_default()
}

fn opt_u32(v: Option<i64>) -> Option<u32> {
    v.and_then(|n| u32::try_from(n).ok())
}

impl Storage {
    /// Opens (creating if needed) the database at `path` and migrates it.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 3000;",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Self::init(conn, Some(path.to_path_buf()))
    }

    /// An in-memory database (tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, None)
    }

    fn init(mut conn: Connection, path: Option<PathBuf>) -> Result<Self> {
        migrate(&mut conn)?;
        let storage = Self { conn: Arc::new(Mutex::new(conn)), path, current_session: Arc::new(Mutex::new(None)) };
        storage.seed_builtin_pricing()?;
        Ok(storage)
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self.conn().query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?)
    }

    // ---- settings -------------------------------------------------------

    /// Loads the settings document; `None` on first run.
    pub fn load_settings(&self) -> Result<Option<Settings>> {
        let value: Option<String> = self
            .conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [SETTINGS_KEY], |r| r.get(0))
            .optional()?;
        match value {
            Some(json) => match serde_json::from_str(&json) {
                Ok(settings) => Ok(Some(settings)),
                Err(error) => {
                    tracing::warn!(%error, "stored settings are unreadable; using defaults");
                    Ok(None)
                }
            },
            None => Ok(None),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let json = serde_json::to_string(settings)?;
        self.conn().execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![SETTINGS_KEY, json, ms(Utc::now())],
        )?;
        Ok(())
    }

    // ---- provider config -------------------------------------------------

    pub fn save_provider_config(&self, provider_id: &str, base_url: &str, config: &serde_json::Value) -> Result<()> {
        self.conn().execute(
            "INSERT INTO provider_config (provider_id, base_url, config, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider_id) DO UPDATE SET base_url = excluded.base_url, config = excluded.config,
             updated_at = excluded.updated_at",
            params![provider_id, base_url, config.to_string(), ms(Utc::now())],
        )?;
        Ok(())
    }

    pub fn load_provider_config(&self, provider_id: &str) -> Result<Option<(String, serde_json::Value)>> {
        let row: Option<(String, String)> = self
            .conn()
            .query_row("SELECT base_url, config FROM provider_config WHERE provider_id = ?1", [provider_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        Ok(match row {
            Some((url, config)) => Some((url, serde_json::from_str(&config)?)),
            None => None,
        })
    }

    // ---- usage -----------------------------------------------------------

    pub fn insert_usage_event(&self, e: &UsageEvent) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO usage_events (ts, provider, model, feature, request_type, input_tokens, output_tokens,
               total_tokens, reasoning_tokens, latency_ms, provider_latency_ms, status, error_kind, attempts, rate_limit_hits)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                ms(e.timestamp),
                e.provider,
                e.model,
                e.feature.as_str(),
                e.request_type.as_str(),
                e.input_tokens,
                e.output_tokens,
                e.total_tokens,
                e.reasoning_tokens,
                e.latency_ms,
                e.provider_latency_ms,
                e.status.as_str(),
                e.error_kind,
                e.attempts,
                e.rate_limit_hits,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Usage aggregated by local day, hour, provider, model, feature and status,
    /// for events on or after the start of local day `since`.
    pub fn usage_buckets(&self, since: NaiveDate) -> Result<Vec<UsageBucket>> {
        let start = Local
            .from_local_datetime(&since.and_hms_opt(0, 0, 0).unwrap_or_default())
            .earliest()
            .map_or(0, |d| d.with_timezone(&Utc).timestamp_millis());
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT strftime('%Y-%m-%d', ts / 1000, 'unixepoch', 'localtime') AS day,
                    CAST(strftime('%H', ts / 1000, 'unixepoch', 'localtime') AS INTEGER) AS hour,
                    provider, model, feature, status,
                    COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(total_tokens), 0), COALESCE(SUM(latency_ms), 0), COUNT(latency_ms),
                    COALESCE(SUM(rate_limit_hits), 0)
             FROM usage_events WHERE ts >= ?1
             GROUP BY day, hour, provider, model, feature, status",
        )?;
        let rows = stmt.query_map([start], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                [r.get::<_, i64>(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?],
            ))
        })?;
        let mut buckets = Vec::new();
        for row in rows {
            let (day, hour, provider, model, feature, status, n) = row?;
            let (Ok(day), Some(feature), Some(status)) =
                (day.parse::<NaiveDate>(), Feature::parse(&feature), UsageStatus::parse(&status))
            else {
                continue;
            };
            let u = |v: i64| u64::try_from(v).unwrap_or(0);
            buckets.push(UsageBucket {
                day,
                hour: u8::try_from(hour).unwrap_or(0),
                provider,
                model,
                feature,
                status,
                requests: u(n[0]),
                input_tokens: u(n[1]),
                output_tokens: u(n[2]),
                total_tokens: u(n[3]),
                latency_sum_ms: u(n[4]),
                latency_count: u(n[5]),
                rate_limit_hits: u(n[6]),
            });
        }
        Ok(buckets)
    }

    /// Latencies of successful requests since `since`, newest first.
    pub fn latency_samples(&self, since: DateTime<Utc>, limit: usize) -> Result<Vec<LatencySample>> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT feature, latency_ms FROM usage_events
             WHERE ts >= ?1 AND status = 'success' AND latency_ms IS NOT NULL
             ORDER BY ts DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![ms(since), i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        let mut samples = Vec::new();
        for row in rows {
            let (feature, latency) = row?;
            if let (Some(feature), Ok(latency_ms)) = (Feature::parse(&feature), u32::try_from(latency)) {
                samples.push(LatencySample { feature, latency_ms });
            }
        }
        Ok(samples)
    }

    /// The most recent usage events, newest first.
    pub fn recent_usage_events(&self, limit: usize) -> Result<Vec<UsageEvent>> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT ts, provider, model, feature, request_type, input_tokens, output_tokens, total_tokens,
                    reasoning_tokens, latency_ms, provider_latency_ms, status, error_kind, attempts, rate_limit_hits
             FROM usage_events ORDER BY ts DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                [r.get::<_, Option<i64>>(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?],
                r.get::<_, String>(11)?,
                r.get::<_, Option<String>>(12)?,
                r.get::<_, i64>(13)?,
                r.get::<_, i64>(14)?,
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (ts, provider, model, feature, request_type, n, status, error_kind, attempts, hits) = row?;
            let (Some(feature), Some(request_type), Some(status)) =
                (Feature::parse(&feature), RequestType::parse(&request_type), UsageStatus::parse(&status))
            else {
                continue;
            };
            events.push(UsageEvent {
                timestamp: from_ms(ts),
                provider,
                model,
                feature,
                request_type,
                input_tokens: opt_u32(n[0]),
                output_tokens: opt_u32(n[1]),
                total_tokens: opt_u32(n[2]),
                reasoning_tokens: opt_u32(n[3]),
                latency_ms: opt_u32(n[4]),
                provider_latency_ms: opt_u32(n[5]),
                status,
                error_kind,
                attempts: u32::try_from(attempts).unwrap_or(1),
                rate_limit_hits: u32::try_from(hits).unwrap_or(0),
            });
        }
        Ok(events)
    }

    pub fn clear_usage(&self) -> Result<u64> {
        Ok(self.conn().execute("DELETE FROM usage_events", [])? as u64)
    }

    pub fn prune_usage(&self, older_than: DateTime<Utc>) -> Result<u64> {
        Ok(self.conn().execute("DELETE FROM usage_events WHERE ts < ?1", [ms(older_than)])? as u64)
    }

    // ---- pricing ---------------------------------------------------------

    /// Inserts built-in prices that are not present yet.
    pub fn seed_builtin_pricing(&self) -> Result<()> {
        let conn = self.conn();
        for p in builtin_pricing() {
            conn.execute(
                "INSERT OR IGNORE INTO model_pricing (provider, model, input_cost_per_million, output_cost_per_million,
                   effective_date, source) VALUES (?1, ?2, ?3, ?4, ?5, 'builtin')",
                params![
                    p.provider,
                    p.model,
                    p.input_cost_per_million,
                    p.output_cost_per_million,
                    p.effective_date.to_string()
                ],
            )?;
        }
        Ok(())
    }

    pub fn list_pricing(&self) -> Result<Vec<ModelPricing>> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT id, provider, model, input_cost_per_million, output_cost_per_million, effective_date, source
             FROM model_pricing ORDER BY provider, model, effective_date",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, provider, model, input, output, date, source) = row?;
            if let Ok(effective_date) = date.parse() {
                out.push(ModelPricing {
                    id: Some(id),
                    provider,
                    model,
                    input_cost_per_million: input,
                    output_cost_per_million: output,
                    effective_date,
                    source: PricingSource::parse(&source),
                });
            }
        }
        Ok(out)
    }

    /// Adds or updates a user price. Built-in rows are never modified; a user
    /// row with the same date takes precedence over them.
    pub fn upsert_pricing(&self, p: &ModelPricing) -> Result<i64> {
        p.validate().map_err(StorageError::Invalid)?;
        let conn = self.conn();
        conn.execute(
            "INSERT INTO model_pricing (provider, model, input_cost_per_million, output_cost_per_million, effective_date, source)
             VALUES (?1, ?2, ?3, ?4, ?5, 'user')
             ON CONFLICT(provider, model, effective_date, source) DO UPDATE SET
               input_cost_per_million = excluded.input_cost_per_million,
               output_cost_per_million = excluded.output_cost_per_million",
            params![p.provider.trim(), p.model.trim(), p.input_cost_per_million, p.output_cost_per_million, p.effective_date.to_string()],
        )?;
        Ok(conn.query_row(
            "SELECT id FROM model_pricing WHERE provider = ?1 AND model = ?2 AND effective_date = ?3 AND source = 'user'",
            params![p.provider.trim(), p.model.trim(), p.effective_date.to_string()],
            |r| r.get(0),
        )?)
    }

    /// Deletes a user price; built-in rows cannot be deleted.
    pub fn delete_pricing(&self, id: i64) -> Result<bool> {
        Ok(self.conn().execute("DELETE FROM model_pricing WHERE id = ?1 AND source = 'user'", [id])? > 0)
    }

    /// Removes all user prices.
    pub fn reset_pricing(&self) -> Result<()> {
        self.conn().execute("DELETE FROM model_pricing WHERE source = 'user'", [])?;
        self.seed_builtin_pricing()
    }

    // ---- exclusions ------------------------------------------------------

    pub fn list_exclusions(&self) -> Result<Vec<ExclusionRule>> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT id, kind, pattern, display_name FROM excluded_apps ORDER BY display_name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, kind, pattern, display_name) = row?;
            let kind = if kind == "window_title" { ExclusionKind::WindowTitle } else { ExclusionKind::App };
            out.push(ExclusionRule { id, kind, pattern, display_name });
        }
        Ok(out)
    }

    pub fn add_exclusion(&self, kind: ExclusionKind, pattern: &str, display_name: &str) -> Result<ExclusionRule> {
        let pattern = pattern.trim();
        let display_name = display_name.trim();
        if pattern.is_empty() || pattern.chars().count() > 200 || display_name.chars().count() > 200 {
            return Err(StorageError::Invalid("Exclusions need a pattern of 1-200 characters.".into()));
        }
        let kind_str = match kind {
            ExclusionKind::App => "app",
            ExclusionKind::WindowTitle => "window_title",
        };
        let name = if display_name.is_empty() { pattern } else { display_name };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO excluded_apps (kind, pattern, display_name, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(kind, pattern) DO UPDATE SET display_name = excluded.display_name",
            params![kind_str, pattern, name, ms(Utc::now())],
        )?;
        let id = conn.query_row(
            "SELECT id FROM excluded_apps WHERE kind = ?1 AND pattern = ?2",
            params![kind_str, pattern],
            |r| r.get(0),
        )?;
        Ok(ExclusionRule { id, kind, pattern: pattern.to_string(), display_name: name.to_string() })
    }

    pub fn remove_exclusion(&self, id: i64) -> Result<bool> {
        Ok(self.conn().execute("DELETE FROM excluded_apps WHERE id = ?1", [id])? > 0)
    }

    // ---- context ---------------------------------------------------------

    /// Persists a metadata event and maintains session bookkeeping.
    pub fn record_context_event(&self, event: &ContextEvent) -> Result<()> {
        let payload = serde_json::to_string(&event.kind)?;
        let ts = ms(event.timestamp);
        let conn = self.conn();
        conn.execute(
            "INSERT INTO context_events (ts, event_type, source, payload) VALUES (?1, ?2, ?3, ?4)",
            params![ts, event.kind.type_name(), event.source, payload],
        )?;
        let mut session = self.current_session.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match &event.kind {
            ContextEventKind::InputFocused { app, role } => {
                if let Some(id) = session.take() {
                    conn.execute("UPDATE context_sessions SET ended_at = ?1 WHERE id = ?2", params![ts, id])?;
                }
                let role = serde_json::to_value(role)?.as_str().unwrap_or("unknown").to_string();
                conn.execute(
                    "INSERT INTO context_sessions (started_at, app_name, input_role) VALUES (?1, ?2, ?3)",
                    params![ts, app, role],
                )?;
                *session = Some(conn.last_insert_rowid());
            }
            ContextEventKind::ApplicationChanged { .. } | ContextEventKind::Paused { .. } => {
                if let Some(id) = session.take() {
                    conn.execute("UPDATE context_sessions SET ended_at = ?1 WHERE id = ?2", params![ts, id])?;
                }
            }
            ContextEventKind::IntentClassified { kind, .. } => {
                if let Some(id) = *session {
                    conn.execute(
                        "UPDATE context_sessions SET intent_kind = ?1 WHERE id = ?2",
                        params![kind.as_str(), id],
                    )?;
                }
            }
            ContextEventKind::SuggestionShown { .. }
            | ContextEventKind::SuggestionAccepted { .. }
            | ContextEventKind::SuggestionDismissed { .. } => {
                let column = match event.kind {
                    ContextEventKind::SuggestionShown { .. } => "suggestions_shown",
                    ContextEventKind::SuggestionAccepted { .. } => "suggestions_accepted",
                    _ => "suggestions_dismissed",
                };
                if let Some(id) = *session {
                    conn.execute(&format!("UPDATE context_sessions SET {column} = {column} + 1 WHERE id = ?1"), [id])?;
                }
            }
            ContextEventKind::ClipboardChanged { .. } | ContextEventKind::Resumed => {}
        }
        Ok(())
    }

    /// Recent context events, newest first.
    pub fn recent_context_events(&self, limit: usize) -> Result<Vec<ContextEvent>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare_cached("SELECT ts, source, payload FROM context_events ORDER BY ts DESC, id DESC LIMIT ?1")?;
        let rows = stmt.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (ts, source, payload) = row?;
            if let Ok(kind) = serde_json::from_str::<ContextEventKind>(&payload) {
                out.push(ContextEvent { timestamp: from_ms(ts), source, kind });
            }
        }
        Ok(out)
    }

    /// Suggestion outcomes for sessions started since `since`.
    pub fn suggestion_stats(&self, since: DateTime<Utc>) -> Result<SuggestionStats> {
        let (shown, accepted, dismissed): (i64, i64, i64) = self.conn().query_row(
            "SELECT COALESCE(SUM(suggestions_shown), 0), COALESCE(SUM(suggestions_accepted), 0),
                    COALESCE(SUM(suggestions_dismissed), 0)
             FROM context_sessions WHERE started_at >= ?1",
            [ms(since)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let u = |v: i64| u64::try_from(v).unwrap_or(0);
        Ok(SuggestionStats { shown: u(shown), accepted: u(accepted), dismissed: u(dismissed) })
    }

    /// Deletes context metadata older than `older_than`.
    pub fn prune_context(&self, older_than: DateTime<Utc>) -> Result<u64> {
        let conn = self.conn();
        let events = conn.execute("DELETE FROM context_events WHERE ts < ?1", [ms(older_than)])?;
        let sessions =
            conn.execute("DELETE FROM context_sessions WHERE COALESCE(ended_at, started_at) < ?1", [ms(older_than)])?;
        Ok((events + sessions) as u64)
    }

    pub fn clear_context(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute_batch("DELETE FROM context_events; DELETE FROM context_sessions;")?;
        *self.current_session.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }

    // ---- preferences -----------------------------------------------------

    pub fn set_preference(&self, scope: &str, key: &str, value: &serde_json::Value) -> Result<()> {
        self.conn().execute(
            "INSERT INTO user_preferences (scope, key, value, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(scope, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![scope, key, value.to_string(), ms(Utc::now())],
        )?;
        Ok(())
    }

    pub fn preference(&self, scope: &str, key: &str) -> Result<Option<serde_json::Value>> {
        let value: Option<String> = self
            .conn()
            .query_row("SELECT value FROM user_preferences WHERE scope = ?1 AND key = ?2", params![scope, key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(match value {
            Some(v) => Some(serde_json::from_str(&v)?),
            None => None,
        })
    }

    // ---- maintenance -----------------------------------------------------

    /// Deletes every row and restores built-in pricing ("Reset local data").
    pub fn reset_all(&self) -> Result<()> {
        {
            let conn = self.conn();
            conn.execute_batch(
                "DELETE FROM settings; DELETE FROM provider_config; DELETE FROM context_events;
                 DELETE FROM context_sessions; DELETE FROM usage_events; DELETE FROM model_pricing;
                 DELETE FROM excluded_apps; DELETE FROM user_preferences;",
            )?;
            *self.current_session.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
        self.seed_builtin_pricing()?;
        self.vacuum()
    }

    /// Reclaims space after deletions.
    pub fn vacuum(&self) -> Result<()> {
        self.conn().execute_batch("VACUUM")?;
        Ok(())
    }

    pub fn stats(&self) -> Result<StorageStats> {
        let conn = self.conn();
        let count = |table: &str| -> Result<u64> {
            let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
            Ok(u64::try_from(n).unwrap_or(0))
        };
        let size = self.path.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len());
        Ok(StorageStats {
            schema_version: conn.query_row("PRAGMA user_version", [], |r| r.get(0))?,
            usage_events: count("usage_events")?,
            context_events: count("context_events")?,
            context_sessions: count("context_sessions")?,
            exclusions: count("excluded_apps")?,
            pricing_rows: count("model_pricing")?,
            size_bytes: size,
        })
    }

    /// Runs SQLite's integrity check.
    pub fn integrity_ok(&self) -> bool {
        self.conn().query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)).map(|s| s == "ok").unwrap_or(false)
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let current: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (index, sql) in migrations::MIGRATIONS.iter().enumerate() {
        let version = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::info!(version, "database migrated");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
