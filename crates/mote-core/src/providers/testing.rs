//! A scripted provider for tests.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::types::*;
use super::{ModelProvider, ProviderError};

/// One scripted reply.
#[derive(Debug, Clone)]
pub struct Script {
    pub outcome: Result<(String, TokenUsage), ProviderError>,
    pub delay: Duration,
}

impl Script {
    pub fn ok(text: &str, input: u32, output: u32) -> Self {
        Self {
            outcome: Ok((
                text.to_string(),
                TokenUsage {
                    input_tokens: input,
                    output_tokens: output,
                    total_tokens: input + output,
                    reasoning_tokens: None,
                },
            )),
            delay: Duration::from_millis(50),
        }
    }

    pub fn fail(error: ProviderError) -> Self {
        Self { outcome: Err(error), delay: Duration::from_millis(20) }
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

/// Replies with scripted outcomes in order and records every request.
/// When the script runs out it echoes "ok".
#[derive(Default)]
pub struct ScriptedProvider {
    scripts: Mutex<VecDeque<Script>>,
    calls: Mutex<Vec<GenerationRequest>>,
    models: Vec<ModelInfo>,
}

impl ScriptedProvider {
    pub fn new(scripts: Vec<Script>) -> Self {
        Self { scripts: Mutex::new(scripts.into()), calls: Mutex::new(Vec::new()), models: Vec::new() }
    }

    pub fn with_models(mut self, ids: &[&str]) -> Self {
        self.models = ids
            .iter()
            .map(|id| ModelInfo {
                id: id.to_string(),
                owned_by: None,
                context_window: None,
                max_output_tokens: None,
                supports_chat: true,
            })
            .collect();
        self
    }

    pub fn push(&self, script: Script) {
        self.scripts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push_back(script);
    }

    pub fn calls(&self) -> Vec<GenerationRequest> {
        self.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

#[async_trait]
impl ModelProvider for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor { id: "scripted", display_name: "Scripted" }
    }

    async fn generate(
        &self,
        request: &GenerationRequest,
        cancel: &CancellationToken,
    ) -> Result<GenerationResponse, ProviderError> {
        self.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(request.clone());
        let script = self
            .scripts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| Script::ok("ok", 1, 1));
        tokio::select! {
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
            _ = tokio::time::sleep(script.delay) => {}
        }
        script.outcome.map(|(text, usage)| GenerationResponse {
            text,
            model: request.model.clone(),
            finish_reason: Some("stop".into()),
            usage: Some(usage),
            latency: script.delay,
            provider_latency: None,
            request_id: None,
            rate_limit: None,
        })
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(self.models.clone())
    }
}
