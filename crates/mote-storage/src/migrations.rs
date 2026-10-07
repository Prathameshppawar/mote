//! Schema migrations. Each entry upgrades the schema by one version and runs
//! in its own transaction. Never edit a shipped migration; append a new one.

pub const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    r#"
    CREATE TABLE settings (
        key        TEXT PRIMARY KEY,
        value      TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE provider_config (
        provider_id TEXT PRIMARY KEY,
        base_url    TEXT NOT NULL,
        config      TEXT NOT NULL,
        updated_at  INTEGER NOT NULL
    );

    -- Metadata about what happened. Never contains typed text, window titles
    -- or clipboard content.
    CREATE TABLE context_events (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        ts         INTEGER NOT NULL,
        event_type TEXT NOT NULL,
        source     TEXT NOT NULL,
        payload    TEXT NOT NULL
    );
    CREATE INDEX idx_context_events_ts ON context_events(ts);

    -- A period of focus on one text input.
    CREATE TABLE context_sessions (
        id                    INTEGER PRIMARY KEY AUTOINCREMENT,
        started_at            INTEGER NOT NULL,
        ended_at              INTEGER,
        app_name              TEXT NOT NULL,
        input_role            TEXT,
        intent_kind           TEXT,
        suggestions_shown     INTEGER NOT NULL DEFAULT 0,
        suggestions_accepted  INTEGER NOT NULL DEFAULT 0,
        suggestions_dismissed INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_context_sessions_started ON context_sessions(started_at);

    -- One row per logical model request. Metadata only.
    CREATE TABLE usage_events (
        id                  INTEGER PRIMARY KEY AUTOINCREMENT,
        ts                  INTEGER NOT NULL,
        provider            TEXT NOT NULL,
        model               TEXT NOT NULL,
        feature             TEXT NOT NULL,
        request_type        TEXT NOT NULL,
        input_tokens        INTEGER,
        output_tokens       INTEGER,
        total_tokens        INTEGER,
        reasoning_tokens    INTEGER,
        latency_ms          INTEGER,
        provider_latency_ms INTEGER,
        status              TEXT NOT NULL,
        error_kind          TEXT,
        attempts            INTEGER NOT NULL DEFAULT 1,
        rate_limit_hits     INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_usage_events_ts ON usage_events(ts);
    CREATE INDEX idx_usage_events_feature_ts ON usage_events(feature, ts);

    CREATE TABLE model_pricing (
        id                       INTEGER PRIMARY KEY AUTOINCREMENT,
        provider                 TEXT NOT NULL,
        model                    TEXT NOT NULL,
        input_cost_per_million   REAL NOT NULL,
        output_cost_per_million  REAL NOT NULL,
        effective_date           TEXT NOT NULL,
        source                   TEXT NOT NULL,
        UNIQUE(provider, model, effective_date, source)
    );

    CREATE TABLE excluded_apps (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        kind         TEXT NOT NULL,
        pattern      TEXT NOT NULL COLLATE NOCASE,
        display_name TEXT NOT NULL,
        created_at   INTEGER NOT NULL,
        UNIQUE(kind, pattern)
    );

    CREATE TABLE user_preferences (
        scope      TEXT NOT NULL,
        key        TEXT NOT NULL,
        value      TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (scope, key)
    );
    "#,
];
