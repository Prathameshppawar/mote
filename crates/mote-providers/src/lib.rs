//! # mote-providers
//!
//! Concrete [`ModelProvider`] implementations.
//!
//! * [`GroqProvider`]: the v1 primary provider (Groq's OpenAI-compatible API).
//! * [`OpenAiCompatibleProvider`]: generic OpenAI-compatible endpoints; the
//!   foundation for future local providers (Ollama, LM Studio).
//!
//! Providers implement single attempts only. Retries, fallback, backoff and
//! usage accounting come from `mote_core::providers::resilient`.

pub mod openai_compat;
pub mod ratelimit;
pub mod secret;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use mote_core::providers::types::*;
use mote_core::providers::{ModelProvider, ProviderError};
pub use openai_compat::{ClientConfig, Dialect, OpenAiCompatibleProvider};
pub use secret::ApiKey;

/// Groq, Mote's v1 provider.
pub struct GroqProvider {
    inner: OpenAiCompatibleProvider,
}

impl GroqProvider {
    pub const ID: &'static str = "groq";
    pub const DISPLAY_NAME: &'static str = "Groq";

    pub fn new(base_url: impl Into<String>, api_key: Option<ApiKey>) -> Result<Self, ProviderError> {
        let config = ClientConfig {
            provider_id: Self::ID,
            display_name: Self::DISPLAY_NAME,
            base_url: base_url.into(),
            dialect: Dialect::Groq,
        };
        Ok(Self { inner: OpenAiCompatibleProvider::new(config, api_key)? })
    }

    pub fn set_api_key(&self, key: Option<ApiKey>) {
        self.inner.set_api_key(key);
    }

    pub fn has_api_key(&self) -> bool {
        self.inner.has_api_key()
    }

    pub fn set_base_url(&self, base_url: String) {
        self.inner.set_base_url(base_url);
    }

    /// Checks a candidate key without storing it ("Test connection" before saving).
    pub async fn verify_key(base_url: &str, key: ApiKey, required_models: &[String]) -> HealthReport {
        match Self::new(base_url, Some(key)) {
            Ok(provider) => provider.health_check(required_models).await,
            Err(error) => HealthReport {
                ok: false,
                latency_ms: None,
                models_available: 0,
                missing_models: Vec::new(),
                message: Some(error.user_message()),
                checked_at: chrono::Utc::now(),
            },
        }
    }
}

#[async_trait]
impl ModelProvider for GroqProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor { id: Self::ID, display_name: Self::DISPLAY_NAME }
    }

    async fn generate(
        &self,
        request: &GenerationRequest,
        cancel: &CancellationToken,
    ) -> Result<GenerationResponse, ProviderError> {
        self.inner.generate(request, cancel).await
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.inner.list_models().await
    }

    fn usage_snapshot(&self) -> Option<RateLimitSnapshot> {
        self.inner.usage_snapshot()
    }
}
