//! The model provider abstraction.
//!
//! ```text
//! ModelProvider (trait)
//! ├── GroqProvider              ← v1 primary   (mote-providers)
//! ├── OpenAiCompatibleProvider  ← generic      (mote-providers)
//! └── LocalProvider             ← future (Ollama, LM Studio, …)
//! ```
//!
//! A provider implements a *single attempt* of each operation. Retries, model
//! fallback, rate-limit backoff, offline detection and usage accounting are
//! layered on top by [`resilient::ResilientProvider`], so every provider gets
//! identical, tested behaviour and usage is recorded exactly once per logical
//! request. Feature-level operations (complete, classify, transform) live in
//! [`crate::ai::AiClient`] and are built on [`ModelProvider::generate`], so a
//! new provider supports all of them without touching assistance logic.

pub mod error;
pub mod resilient;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod types;

use std::collections::HashSet;

use async_trait::async_trait;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

pub use error::ProviderError;
pub use types::*;

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;

    /// generate(): performs one generation attempt.
    ///
    /// Implementations should stop work promptly when `cancel` fires; dropping
    /// the returned future must also be safe.
    async fn generate(
        &self,
        request: &GenerationRequest,
        cancel: &CancellationToken,
    ) -> Result<GenerationResponse, ProviderError>;

    /// Models offered to this account.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    /// healthCheck(): verifies credentials and that `required_models` exist.
    async fn health_check(&self, required_models: &[String]) -> HealthReport {
        let started = std::time::Instant::now();
        match self.list_models().await {
            Ok(models) => {
                let available: HashSet<&str> = models.iter().map(|m| m.id.as_str()).collect();
                let mut missing: Vec<String> =
                    required_models.iter().filter(|m| !available.contains(m.as_str())).cloned().collect();
                missing.sort();
                missing.dedup();
                let message = (!missing.is_empty()).then(|| {
                    format!(
                        "Connected, but these configured models are unavailable: {}. Choose others in Settings → Models.",
                        missing.join(", ")
                    )
                });
                HealthReport {
                    ok: missing.is_empty(),
                    latency_ms: Some(started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32),
                    models_available: models.iter().filter(|m| m.supports_chat).count() as u32,
                    missing_models: missing,
                    message,
                    checked_at: Utc::now(),
                }
            }
            Err(error) => HealthReport {
                ok: false,
                latency_ms: None,
                models_available: 0,
                missing_models: Vec::new(),
                message: Some(error.user_message()),
                checked_at: Utc::now(),
            },
        }
    }

    /// getUsage(): provider-reported usage and limits, when exposed.
    fn usage_snapshot(&self) -> Option<RateLimitSnapshot> {
        None
    }
}
