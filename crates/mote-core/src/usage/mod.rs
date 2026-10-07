//! Local, privacy-preserving usage accounting.
//!
//! Every model request produces one [`UsageEvent`] containing metadata only:
//! provider, model, feature, token counts, latency and outcome. Prompt text,
//! completions and clipboard content are never part of a usage event.

pub mod dashboard;
pub mod pricing;

use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::providers::types::{Feature, RequestType};

/// Final outcome of a logical request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum UsageStatus {
    Success,
    Error,
    RateLimited,
    Timeout,
    Cancelled,
}

impl UsageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
            Self::RateLimited => "rate_limited",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Success, Self::Error, Self::RateLimited, Self::Timeout, Self::Cancelled]
            .into_iter()
            .find(|v| v.as_str() == s)
    }

    /// Whether the request failed (cancellation is not a failure).
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Error | Self::RateLimited | Self::Timeout)
    }
}

/// One logical model request. Metadata only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub struct UsageEvent {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub feature: Feature,
    pub request_type: RequestType,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
    /// End-to-end latency measured by Mote, including retries.
    pub latency_ms: Option<u32>,
    /// Processing time reported by the provider.
    pub provider_latency_ms: Option<u32>,
    pub status: UsageStatus,
    pub error_kind: Option<String>,
    /// HTTP attempts made, including retries and model fallbacks.
    pub attempts: u32,
    /// HTTP 429 responses encountered while serving this request.
    pub rate_limit_hits: u32,
}

/// Receives usage events. Implementations must not block the caller.
pub trait UsageSink: Send + Sync {
    fn record(&self, event: UsageEvent);
}

/// Discards events (usage analytics disabled).
#[derive(Debug, Default)]
pub struct NullUsageSink;

impl UsageSink for NullUsageSink {
    fn record(&self, _event: UsageEvent) {}
}

/// Keeps events in memory (tests, diagnostics).
#[derive(Debug, Default)]
pub struct MemoryUsageSink {
    events: Mutex<Vec<UsageEvent>>,
}

impl MemoryUsageSink {
    pub fn events(&self) -> Vec<UsageEvent> {
        self.events.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

impl UsageSink for MemoryUsageSink {
    fn record(&self, event: UsageEvent) {
        self.events.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_in_documented_shape_without_content_fields() {
        let event = UsageEvent {
            timestamp: "2026-10-07T10:00:00Z".parse().unwrap(),
            provider: "groq".into(),
            model: "qwen/qwen3.8-27b".into(),
            feature: Feature::InlineCompletion,
            request_type: RequestType::Completion,
            input_tokens: Some(123),
            output_tokens: Some(31),
            total_tokens: Some(154),
            reasoning_tokens: None,
            latency_ms: Some(420),
            provider_latency_ms: Some(30),
            status: UsageStatus::Success,
            error_kind: None,
            attempts: 1,
            rate_limit_hits: 0,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["feature"], "inline_completion");
        assert_eq!(json["request_type"], "completion");
        assert_eq!(json["total_tokens"], 154);
        assert_eq!(json["status"], "success");
        for forbidden in ["prompt", "text", "content", "completion_text", "messages"] {
            assert!(json.get(forbidden).is_none(), "usage events must not carry {forbidden}");
        }
    }

    #[test]
    fn failure_classification() {
        assert!(UsageStatus::Error.is_failure());
        assert!(UsageStatus::RateLimited.is_failure());
        assert!(UsageStatus::Timeout.is_failure());
        assert!(!UsageStatus::Cancelled.is_failure());
        assert!(!UsageStatus::Success.is_failure());
        assert_eq!(UsageStatus::parse("rate_limited"), Some(UsageStatus::RateLimited));
    }
}
