//! Provider errors and how Mote reacts to them.

use std::time::Duration;

use thiserror::Error;

use super::types::RateLimitSnapshot;
use crate::usage::UsageStatus;

#[derive(Debug, Clone, Error, PartialEq)]
pub enum ProviderError {
    #[error("no API key is configured")]
    NotConfigured,
    #[error("cloud AI is turned off in privacy settings")]
    CloudDisabled,
    #[error("the API key was rejected")]
    Unauthorized,
    #[error("rate limited by the provider")]
    RateLimited { retry_after: Option<Duration>, snapshot: Option<Box<RateLimitSnapshot>> },
    #[error("model `{model}` is unavailable")]
    ModelUnavailable { model: String },
    #[error("the request timed out")]
    Timeout,
    #[error("network error: {0}")]
    Network(String),
    #[error("the request was cancelled")]
    Cancelled,
    #[error("provider error (HTTP {status}): {message}")]
    Server { status: u16, message: String },
    #[error("request rejected (HTTP {status}): {message}")]
    BadRequest { status: u16, message: String },
    #[error("unexpected response: {0}")]
    InvalidResponse(String),
}

impl ProviderError {
    /// Stable identifier recorded in usage events (never contains content).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::CloudDisabled => "cloud_disabled",
            Self::Unauthorized => "unauthorized",
            Self::RateLimited { .. } => "rate_limited",
            Self::ModelUnavailable { .. } => "model_unavailable",
            Self::Timeout => "timeout",
            Self::Network(_) => "network",
            Self::Cancelled => "cancelled",
            Self::Server { .. } => "server",
            Self::BadRequest { .. } => "bad_request",
            Self::InvalidResponse(_) => "invalid_response",
        }
    }

    /// Transient failures worth retrying (rate limits are handled separately).
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) | Self::Timeout => true,
            Self::Server { status, .. } => matches!(status, 500 | 502 | 503 | 504 | 520..=530),
            _ => false,
        }
    }

    /// Whether the request reached the provider at all (for usage accounting).
    pub fn reached_provider(&self) -> bool {
        !matches!(self, Self::NotConfigured | Self::CloudDisabled)
    }

    pub fn usage_status(&self) -> UsageStatus {
        match self {
            Self::RateLimited { .. } => UsageStatus::RateLimited,
            Self::Timeout => UsageStatus::Timeout,
            Self::Cancelled => UsageStatus::Cancelled,
            _ => UsageStatus::Error,
        }
    }

    /// Short, actionable message for the UI.
    pub fn user_message(&self) -> String {
        match self {
            Self::NotConfigured => "Add your Groq API key in Settings → AI Providers.".into(),
            Self::CloudDisabled => "Cloud AI is turned off in Settings → Privacy.".into(),
            Self::Unauthorized => {
                "Groq rejected the API key. Check it in Settings → AI Providers, or create a new key in the Groq console."
                    .into()
            }
            Self::RateLimited { retry_after, .. } => match retry_after {
                Some(wait) => format!("Groq rate limit reached. Mote will try again in about {}s.", wait.as_secs().max(1)),
                None => "Groq rate limit reached. Mote will try again shortly.".into(),
            },
            Self::ModelUnavailable { model } => {
                format!("The model \"{model}\" is unavailable for this API key. Choose another model in Settings → Models.")
            }
            Self::Timeout => "Groq did not respond in time. Check your connection; Mote will keep trying.".into(),
            Self::Network(_) => "Can't reach Groq. Cloud assistance is paused until the connection returns.".into(),
            Self::Cancelled => "Cancelled.".into(),
            Self::Server { status, .. } => format!("Groq had a problem (HTTP {status}). Mote will retry."),
            Self::BadRequest { message, .. } => {
                format!("Groq rejected the request: {}", crate::privacy::redact::redact_secrets(message))
            }
            Self::InvalidResponse(_) => "Groq sent a response Mote could not read.".into(),
        }
    }
}
