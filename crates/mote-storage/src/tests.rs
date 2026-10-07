use chrono::{Duration, Local, TimeZone, Timelike};

use super::*;
use mote_core::context::clipboard::ClipboardKind;
use mote_core::intent::IntentKind;
use mote_core::platform::InputRole;
use mote_core::usage::dashboard::{build, DashboardInput};
use mote_core::usage::pricing::PricingCatalog;

fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
    Local.with_ymd_and_hms(y, mo, d, h, mi, 0).earliest().unwrap().with_timezone(&Utc)
}

#[allow(clippy::too_many_arguments)]
fn event(
    at: DateTime<Utc>,
    model: &str,
    feature: Feature,
    status: UsageStatus,
    input: Option<u32>,
    output: Option<u32>,
    latency: Option<u32>,
    hits: u32,
) -> UsageEvent {
    UsageEvent {
        timestamp: at,
        provider: "groq".into(),
        model: model.into(),
        feature,
        request_type: RequestType::Completion,
        input_tokens: input,
        output_tokens: output,
        total_tokens: match (input, output) {
            (Some(i), Some(o)) => Some(i + o),
            _ => None,
        },
        reasoning_tokens: None,
        latency_ms: latency,
        provider_latency_ms: None,
        status,
        error_kind: status.is_failure().then(|| "server".to_string()),
        attempts: 1,
        rate_limit_hits: hits,
    }
}

const QWEN: &str = "qwen/qwen3.8-27b";
const OSS: &str = "openai/gpt-oss-120b";

#[test]
fn migrations_are_idempotent_and_files_are_private() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/mote.db");
    let s = Storage::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 1);
    drop(s);
    let s = Storage::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), 1);
    assert!(s.integrity_ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    assert!(s.stats().unwrap().size_bytes.unwrap() > 0);
}

#[test]
fn settings_roundtrip_and_corruption_recovery() {
    let s = Storage::open_in_memory().unwrap();
    assert_eq!(s.load_settings().unwrap(), None);
    let mut settings = Settings::default();
    settings.completion.debounce_ms = 600;
    settings.writing.ignored_words = vec!["mote".into()];
    s.save_settings(&settings).unwrap();
    assert_eq!(s.load_settings().unwrap(), Some(settings.clone()));
    s.conn().execute("UPDATE settings SET value = 'not json'", []).unwrap();
    assert_eq!(s.load_settings().unwrap(), None, "unreadable settings fall back to defaults");
}

#[test]
fn usage_events_roundtrip_every_field() {
    let s = Storage::open_in_memory().unwrap();
    let mut e =
        event(Utc::now(), QWEN, Feature::InlineCompletion, UsageStatus::Success, Some(123), Some(31), Some(420), 0);
    e.reasoning_tokens = Some(5);
    e.provider_latency_ms = Some(30);
    e.attempts = 2;
    e.request_type = RequestType::Completion;
    s.insert_usage_event(&e).unwrap();
    let back = s.recent_usage_events(10).unwrap();
    assert_eq!(back.len(), 1);
    let mut expected = e.clone();
    expected.timestamp = from_ms(ms(e.timestamp));
    assert_eq!(back[0], expected);
}

#[test]
fn buckets_aggregate_by_local_day_hour_feature_model_and_status() {
    let s = Storage::open_in_memory().unwrap();
    let morning = local(2026, 10, 7, 9, 10);
    let morning_later = local(2026, 10, 7, 9, 50);
    let afternoon = local(2026, 10, 7, 14, 5);
    let yesterday = local(2026, 10, 6, 23, 30);
    for e in [
        event(morning, QWEN, Feature::InlineCompletion, UsageStatus::Success, Some(100), Some(10), Some(300), 0),
        event(morning_later, QWEN, Feature::InlineCompletion, UsageStatus::Success, Some(120), Some(12), Some(500), 0),
        event(morning_later, QWEN, Feature::InlineCompletion, UsageStatus::Cancelled, None, None, Some(200), 0),
        event(afternoon, OSS, Feature::PromptEnhancement, UsageStatus::Success, Some(300), Some(200), Some(1_200), 1),
        event(afternoon, QWEN, Feature::InlineCompletion, UsageStatus::RateLimited, None, None, Some(90), 1),
        event(afternoon, QWEN, Feature::WritingAssistance, UsageStatus::Error, None, None, Some(80), 0),
        event(yesterday, QWEN, Feature::IntentClassification, UsageStatus::Success, Some(50), Some(5), Some(250), 0),
    ] {
        s.insert_usage_event(&e).unwrap();
    }
    let buckets = s.usage_buckets(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()).unwrap();
    let find = |day: &str, hour: u8, feature: Feature, status: UsageStatus| {
        buckets
            .iter()
            .find(|b| b.day.to_string() == day && b.hour == hour && b.feature == feature && b.status == status)
            .cloned()
            .unwrap_or_else(|| panic!("missing bucket {day} {hour} {feature:?} {status:?} in {buckets:?}"))
    };
    let b = find("2026-10-07", 9, Feature::InlineCompletion, UsageStatus::Success);
    assert_eq!((b.requests, b.input_tokens, b.output_tokens, b.total_tokens), (2, 220, 22, 242));
    assert_eq!((b.latency_sum_ms, b.latency_count), (800, 2));
    let cancelled = find("2026-10-07", 9, Feature::InlineCompletion, UsageStatus::Cancelled);
    assert_eq!((cancelled.requests, cancelled.total_tokens), (1, 0));
    let limited = find("2026-10-07", 14, Feature::InlineCompletion, UsageStatus::RateLimited);
    assert_eq!(limited.rate_limit_hits, 1);
    let enhancement = find("2026-10-07", 14, Feature::PromptEnhancement, UsageStatus::Success);
    assert_eq!((enhancement.model.as_str(), enhancement.rate_limit_hits), (OSS, 1));
    let y = find("2026-10-06", 23, Feature::IntentClassification, UsageStatus::Success);
    assert_eq!(y.requests, 1);
    // Events before `since` are excluded.
    assert!(s
        .usage_buckets(NaiveDate::from_ymd_opt(2026, 10, 7).unwrap())
        .unwrap()
        .iter()
        .all(|b| b.day.to_string() == "2026-10-07"));
}

#[test]
fn dashboard_end_to_end_from_stored_events() {
    let s = Storage::open_in_memory().unwrap();
    let today = Local::now();
    let now = today.with_timezone(&Utc);
    // Stay inside today even when the test runs just after midnight.
    let start_of_today =
        Local.from_local_datetime(&today.date_naive().and_hms_opt(0, 0, 1).unwrap()).earliest().unwrap();
    let earlier_today = std::cmp::max(today - Duration::minutes(5), start_of_today).with_timezone(&Utc);
    s.insert_usage_event(&event(
        earlier_today,
        QWEN,
        Feature::InlineCompletion,
        UsageStatus::Success,
        Some(1_000),
        Some(250),
        Some(400),
        0,
    ))
    .unwrap();
    s.insert_usage_event(&event(
        earlier_today,
        QWEN,
        Feature::InlineCompletion,
        UsageStatus::Timeout,
        None,
        None,
        Some(20_000),
        0,
    ))
    .unwrap();
    s.insert_usage_event(&event(
        earlier_today,
        QWEN,
        Feature::InlineCompletion,
        UsageStatus::Cancelled,
        None,
        None,
        Some(100),
        0,
    ))
    .unwrap();
    s.insert_usage_event(&event(
        earlier_today,
        OSS,
        Feature::PromptEnhancement,
        UsageStatus::Success,
        Some(500),
        Some(500),
        Some(900),
        2,
    ))
    .unwrap();

    let local_today = today.date_naive();
    let buckets = s.usage_buckets(local_today - Duration::days(30)).unwrap();
    let latencies = s.latency_samples(now - Duration::days(30), 10_000).unwrap();
    assert_eq!(latencies.len(), 2, "only successful requests contribute latency samples");
    let pricing = PricingCatalog::new(s.list_pricing().unwrap());
    let dash = build(DashboardInput {
        buckets: &buckets,
        latencies: &latencies,
        pricing: &pricing,
        today: local_today,
        current_hour: u8::try_from(today.hour()).unwrap(),
        provider_limits: None,
        generated_at: now,
    });
    assert_eq!(dash.today.requests, 4);
    assert_eq!(dash.today.successful, 2);
    assert_eq!(dash.today.failed, 1);
    assert_eq!(dash.today.timeouts, 1);
    assert_eq!(dash.today.cancelled, 1);
    assert_eq!(dash.today.input_tokens, 1_500);
    assert_eq!(dash.today.output_tokens, 750);
    assert_eq!(dash.today.total_tokens, 2_250);
    let expected_cost = (1_000.0 * 0.80 + 250.0 * 4.0 + 500.0 * 0.15 + 500.0 * 0.60) / 1e6;
    assert!((dash.today.estimated_cost_usd - expected_cost).abs() < 1e-12, "{}", dash.today.estimated_cost_usd);
    assert!((dash.today.error_rate - 1.0 / 3.0).abs() < 1e-9);
    assert_eq!(dash.breakdown_30d.performance.rate_limit_events, 2);
    assert_eq!(dash.breakdown_30d.performance.completion_median_latency_ms, Some(400.0));
    assert_eq!(dash.month.requests, 4);
}

#[test]
fn pruning_and_clearing_usage() {
    let s = Storage::open_in_memory().unwrap();
    let now = Utc::now();
    s.insert_usage_event(&event(
        now - Duration::days(200),
        QWEN,
        Feature::InlineCompletion,
        UsageStatus::Success,
        Some(1),
        Some(1),
        Some(1),
        0,
    ))
    .unwrap();
    s.insert_usage_event(&event(
        now,
        QWEN,
        Feature::InlineCompletion,
        UsageStatus::Success,
        Some(1),
        Some(1),
        Some(1),
        0,
    ))
    .unwrap();
    assert_eq!(s.prune_usage(now - Duration::days(180)).unwrap(), 1);
    assert_eq!(s.stats().unwrap().usage_events, 1);
    assert_eq!(s.clear_usage().unwrap(), 1);
    assert_eq!(s.stats().unwrap().usage_events, 0);
}

#[test]
fn pricing_is_seeded_once_and_user_rows_override() {
    let s = Storage::open_in_memory().unwrap();
    s.seed_builtin_pricing().unwrap();
    let rows = s.list_pricing().unwrap();
    assert_eq!(rows.len(), 3, "seeding is idempotent");
    let builtin = rows.iter().find(|r| r.model == QWEN).unwrap().clone();
    assert!(!s.delete_pricing(builtin.id.unwrap()).unwrap(), "built-in rows cannot be deleted");

    let mut user = builtin.clone();
    user.input_cost_per_million = 0.5;
    user.source = PricingSource::User;
    let id = s.upsert_pricing(&user).unwrap();
    user.input_cost_per_million = 0.4;
    assert_eq!(s.upsert_pricing(&user).unwrap(), id, "same date updates in place");
    let catalog = PricingCatalog::new(s.list_pricing().unwrap());
    assert_eq!(catalog.price_at("groq", QWEN, builtin.effective_date).unwrap().input_cost_per_million, 0.4);

    let mut invalid = user.clone();
    invalid.output_cost_per_million = -3.0;
    assert!(matches!(s.upsert_pricing(&invalid), Err(StorageError::Invalid(_))));

    s.reset_pricing().unwrap();
    assert_eq!(s.list_pricing().unwrap().len(), 3);
    assert!(s.list_pricing().unwrap().iter().all(|r| r.source == PricingSource::Builtin));
}

#[test]
fn exclusions_crud() {
    let s = Storage::open_in_memory().unwrap();
    let a = s.add_exclusion(ExclusionKind::App, "com.example.Bank", "Bank").unwrap();
    let again = s.add_exclusion(ExclusionKind::App, "COM.EXAMPLE.BANK", "My Bank").unwrap();
    assert_eq!(a.id, again.id, "patterns are case-insensitive");
    s.add_exclusion(ExclusionKind::WindowTitle, "NetBanking", "").unwrap();
    let rules = s.list_exclusions().unwrap();
    assert_eq!(rules.len(), 2);
    assert!(rules.iter().any(|r| r.kind == ExclusionKind::WindowTitle && r.display_name == "NetBanking"));
    assert!(s.add_exclusion(ExclusionKind::App, "  ", "x").is_err());
    assert!(s.remove_exclusion(a.id).unwrap());
    assert!(!s.remove_exclusion(a.id).unwrap());
    assert_eq!(s.list_exclusions().unwrap().len(), 1);
}

#[test]
fn context_events_sessions_and_retention() {
    let s = Storage::open_in_memory().unwrap();
    let t0 = Utc::now() - Duration::minutes(10);
    let ev = |offset: i64, kind: ContextEventKind| ContextEvent {
        timestamp: t0 + Duration::seconds(offset),
        source: "macos".into(),
        kind,
    };
    for e in [
        ev(
            0,
            ContextEventKind::ApplicationChanged {
                from: None,
                to: "Slack".into(),
                category: mote_core::intent::apps::AppCategory::Chat,
            },
        ),
        ev(1, ContextEventKind::InputFocused { app: "Slack".into(), role: InputRole::TextArea }),
        ev(
            2,
            ContextEventKind::IntentClassified { app: "Slack".into(), kind: IntentKind::Conversation, confidence: 0.9 },
        ),
        ev(3, ContextEventKind::SuggestionShown { feature: Feature::InlineCompletion }),
        ev(4, ContextEventKind::SuggestionAccepted { feature: Feature::InlineCompletion }),
        ev(5, ContextEventKind::SuggestionShown { feature: Feature::WritingAssistance }),
        ev(6, ContextEventKind::SuggestionDismissed { feature: Feature::WritingAssistance }),
        ev(
            7,
            ContextEventKind::ClipboardChanged {
                source_app: Some("Slack".into()),
                kind: ClipboardKind::Text,
                char_count: 120,
            },
        ),
    ] {
        s.record_context_event(&e).unwrap();
    }
    let recent = s.recent_context_events(100).unwrap();
    assert_eq!(recent.len(), 8);
    assert_eq!(recent[0].kind.type_name(), "clipboard_changed");
    let stats = s.suggestion_stats(t0 - Duration::minutes(1)).unwrap();
    assert_eq!(stats, SuggestionStats { shown: 2, accepted: 1, dismissed: 1 });
    // Nothing stored contains content.
    let payloads: Vec<String> = s
        .conn()
        .prepare("SELECT payload FROM context_events")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert!(payloads.iter().all(|p| !p.contains("text\":")));

    assert_eq!(s.prune_context(Utc::now()).unwrap(), 9, "8 events + 1 session");
    assert!(s.recent_context_events(10).unwrap().is_empty());
    s.record_context_event(&ev(8, ContextEventKind::Resumed)).unwrap();
    s.clear_context().unwrap();
    assert_eq!(s.stats().unwrap().context_events, 0);
}

#[test]
fn preferences_and_provider_config() {
    let s = Storage::open_in_memory().unwrap();
    s.set_preference("app:com.tinyspeck.slackmacgap", "intent", &serde_json::json!("conversation")).unwrap();
    assert_eq!(
        s.preference("app:com.tinyspeck.slackmacgap", "intent").unwrap(),
        Some(serde_json::json!("conversation"))
    );
    assert_eq!(s.preference("x", "y").unwrap(), None);
    s.save_provider_config("groq", "https://api.groq.com/openai/v1", &serde_json::json!({"models": 1})).unwrap();
    let (url, config) = s.load_provider_config("groq").unwrap().unwrap();
    assert_eq!(url, "https://api.groq.com/openai/v1");
    assert_eq!(config["models"], 1);
}

#[test]
fn reset_all_removes_everything_but_builtin_pricing() {
    let s = Storage::open_in_memory().unwrap();
    s.save_settings(&Settings::default()).unwrap();
    s.insert_usage_event(&event(
        Utc::now(),
        QWEN,
        Feature::InlineCompletion,
        UsageStatus::Success,
        Some(1),
        Some(1),
        Some(1),
        0,
    ))
    .unwrap();
    s.add_exclusion(ExclusionKind::App, "x", "x").unwrap();
    s.record_context_event(&ContextEvent {
        timestamp: Utc::now(),
        source: "mote".into(),
        kind: ContextEventKind::Resumed,
    })
    .unwrap();
    s.reset_all().unwrap();
    let stats = s.stats().unwrap();
    assert_eq!((stats.usage_events, stats.context_events, stats.exclusions, stats.pricing_rows), (0, 0, 0, 3));
    assert_eq!(s.load_settings().unwrap(), None);
}
