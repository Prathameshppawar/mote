//! Provider-neutral request and response types.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Logical model roles. Each role is mapped to a concrete model in settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    /// Inline completion: latency first, short outputs.
    Completion,
    /// Intent classification: short structured outputs.
    Classification,
    /// Spelling, grammar, tone and rewriting.
    Writing,
    /// Prompt enhancement and analysis: quality first.
    Reasoning,
}

/// The Mote feature a request serves; recorded with every usage event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    InlineCompletion,
    WritingAssistance,
    PromptEnhancement,
    IntentClassification,
    ContextAnalysis,
    Translation,
    Rewrite,
    CommandInterface,
}

impl Feature {
    pub const ALL: [Feature; 8] = [
        Self::InlineCompletion,
        Self::WritingAssistance,
        Self::PromptEnhancement,
        Self::IntentClassification,
        Self::ContextAnalysis,
        Self::Translation,
        Self::Rewrite,
        Self::CommandInterface,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InlineCompletion => "inline_completion",
            Self::WritingAssistance => "writing_assistance",
            Self::PromptEnhancement => "prompt_enhancement",
            Self::IntentClassification => "intent_classification",
            Self::ContextAnalysis => "context_analysis",
            Self::Translation => "translation",
            Self::Rewrite => "rewrite",
            Self::CommandInterface => "command_interface",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_str() == s)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::InlineCompletion => "Inline Completion",
            Self::WritingAssistance => "Writing Assistance",
            Self::PromptEnhancement => "Prompt Enhancement",
            Self::IntentClassification => "Classification",
            Self::ContextAnalysis => "Context Analysis",
            Self::Translation => "Translation",
            Self::Rewrite => "Rewrite",
            Self::CommandInterface => "Command Interface",
        }
    }
}

/// The shape of a request, for usage analytics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RequestType {
    Completion,
    Classification,
    Transform,
    Generation,
}

impl RequestType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completion => "completion",
            Self::Classification => "classification",
            Self::Transform => "transform",
            Self::Generation => "generation",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Completion, Self::Classification, Self::Transform, Self::Generation]
            .into_iter()
            .find(|r| r.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: ChatRole::System, content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: ChatRole::User, content: content.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ResponseFormat {
    #[default]
    Text,
    JsonObject,
}

/// How much hidden reasoning the model may do. Providers map this onto
/// model-specific parameters; `None` disables reasoning where the model allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReasoningEffort {
    #[default]
    None,
    Low,
    Medium,
    High,
}

/// Retry behaviour for one logical request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Attempts per model, including the first.
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    /// Whether to wait out a 429 and retry.
    pub retry_rate_limits: bool,
    /// Longest `retry-after` worth waiting for.
    pub max_rate_limit_wait: Duration,
}

impl RetryPolicy {
    /// No retries: inline completion, where a late answer is useless.
    pub const fn none() -> Self {
        Self {
            max_attempts: 1,
            base_delay: Duration::from_millis(0),
            max_delay: Duration::from_millis(0),
            retry_rate_limits: false,
            max_rate_limit_wait: Duration::from_millis(0),
        }
    }

    /// One quick retry for background work such as classification.
    pub const fn background() -> Self {
        Self {
            max_attempts: 2,
            base_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(1),
            retry_rate_limits: false,
            max_rate_limit_wait: Duration::from_millis(0),
        }
    }

    /// User-initiated actions: retry transient failures and short rate limits.
    pub const fn interactive() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(400),
            max_delay: Duration::from_secs(4),
            retry_rate_limits: true,
            max_rate_limit_wait: Duration::from_secs(8),
        }
    }

    /// Exponential backoff delay before attempt `attempt + 1` (`attempt` ≥ 1).
    pub fn delay_for(&self, attempt: u32) -> Duration {
        let factor = 2u32.saturating_pow(attempt.saturating_sub(1));
        self.base_delay.saturating_mul(factor).min(self.max_delay)
    }
}

/// One generation request.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationRequest {
    pub model: String,
    /// Tried in order when `model` is unavailable.
    pub fallback_models: Vec<String>,
    pub messages: Vec<ChatMessage>,
    /// Visible output budget. Providers add headroom for hidden reasoning.
    pub max_output_tokens: u32,
    pub temperature: f32,
    pub stop: Vec<String>,
    pub response_format: ResponseFormat,
    pub reasoning: ReasoningEffort,
    /// Per-attempt timeout.
    pub timeout: Duration,
    pub feature: Feature,
    pub request_type: RequestType,
    pub retry: RetryPolicy,
}

/// Token counts reported by the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    /// Hidden reasoning tokens (already included in `output_tokens`).
    pub reasoning_tokens: Option<u32>,
}

/// Rate-limit state reported by the provider in response headers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSnapshot {
    /// For Groq: requests per day.
    pub requests_limit: Option<u64>,
    pub requests_remaining: Option<u64>,
    pub requests_reset_secs: Option<f64>,
    /// For Groq: tokens per minute.
    pub tokens_limit: Option<u64>,
    pub tokens_remaining: Option<u64>,
    pub tokens_reset_secs: Option<f64>,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub observed_at: DateTime<Utc>,
}

/// A successful generation.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationResponse {
    pub text: String,
    /// The model that actually served the request.
    pub model: String,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
    /// Wall-clock latency measured by Mote.
    pub latency: Duration,
    /// Processing time reported by the provider, if any.
    pub provider_latency: Option<Duration>,
    pub request_id: Option<String>,
    pub rate_limit: Option<RateLimitSnapshot>,
}

/// A model offered by the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub owned_by: Option<String>,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    /// Whether the model accepts chat completions (not speech/guard models).
    pub supports_chat: bool,
}

/// Result of a provider health check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub ok: bool,
    pub latency_ms: Option<u32>,
    pub models_available: u32,
    /// Configured models the provider does not offer.
    pub missing_models: Vec<String>,
    /// Actionable, user-facing message when `ok` is false.
    pub message: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub checked_at: DateTime<Utc>,
}

/// Static description of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
}
