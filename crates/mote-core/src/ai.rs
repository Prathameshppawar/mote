//! Feature-level AI operations: complete(), classify(), transform().
//!
//! [`AiClient`] maps each operation to a model role, builds the prompt, picks
//! token budgets and retry policies, and runs the request through the
//! [`ResilientProvider`] (which meters usage). Because these operations are
//! built on [`crate::providers::ModelProvider::generate`], any provider that
//! implements `generate` supports all of them.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::completion::clean_completion;
use crate::intent::{parse_ai_classification, IntentAssessment};
use crate::language::LanguageProfile;
use crate::prompts::{self, ClassificationPrompt, CompletionPrompt, TransformAction, TransformPrompt};
use crate::providers::resilient::ResilientProvider;
use crate::providers::types::*;
use crate::providers::ProviderError;
use crate::settings::ModelAssignments;

/// How requests are routed: which model serves each role.
#[derive(Debug, Clone, PartialEq)]
pub struct Routing {
    pub models: ModelAssignments,
    pub timeout: Duration,
    /// Privacy switch: when false, nothing is sent to the provider.
    pub cloud_enabled: bool,
    /// Whether credentials are configured.
    pub configured: bool,
}

impl Default for Routing {
    fn default() -> Self {
        Self {
            models: ModelAssignments::default(),
            timeout: Duration::from_secs(20),
            cloud_enabled: true,
            configured: false,
        }
    }
}

pub struct AiClient {
    provider: Arc<ResilientProvider>,
    routing: RwLock<Routing>,
}

impl AiClient {
    pub fn new(provider: Arc<ResilientProvider>, routing: Routing) -> Self {
        Self { provider, routing: RwLock::new(routing) }
    }

    pub fn provider(&self) -> &Arc<ResilientProvider> {
        &self.provider
    }

    pub fn routing(&self) -> Routing {
        self.routing.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub fn set_routing(&self, routing: Routing) {
        *self.routing.write().unwrap_or_else(std::sync::PoisonError::into_inner) = routing;
    }

    /// Whether automatic (background) features may call the provider now.
    pub fn automatic_requests_allowed(&self) -> bool {
        let routing = self.routing();
        routing.cloud_enabled
            && routing.configured
            && self.provider.rate_limited_until().is_none()
            && !self.provider.is_offline()
            && !self.provider.is_unauthorized()
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        &self,
        role: ModelRole,
        feature: Feature,
        request_type: RequestType,
        messages: Vec<ChatMessage>,
        max_output_tokens: u32,
        temperature: f32,
        response_format: ResponseFormat,
        reasoning: ReasoningEffort,
        retry: RetryPolicy,
        stop: Vec<String>,
    ) -> Result<GenerationRequest, ProviderError> {
        let routing = self.routing();
        if !routing.cloud_enabled {
            return Err(ProviderError::CloudDisabled);
        }
        if !routing.configured {
            return Err(ProviderError::NotConfigured);
        }
        let models = &routing.models;
        let model = match role {
            ModelRole::Completion => &models.completion,
            ModelRole::Classification => &models.classification,
            ModelRole::Writing => &models.writing,
            ModelRole::Reasoning => &models.reasoning,
        };
        Ok(GenerationRequest {
            model: model.clone(),
            fallback_models: vec![models.fallback.clone()],
            messages,
            max_output_tokens,
            temperature,
            stop,
            response_format,
            reasoning,
            timeout: routing.timeout,
            feature,
            request_type,
            retry,
        })
    }

    /// complete(): an inline continuation, cleaned for insertion.
    pub async fn complete(
        &self,
        p: &CompletionPrompt<'_>,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, ProviderError> {
        let max_tokens = (p.max_words * 2 + 8).min(96);
        let temperature = if p.avoid.is_empty() { 0.3 } else { 0.8 };
        let request = self.build(
            ModelRole::Completion,
            Feature::InlineCompletion,
            RequestType::Completion,
            prompts::completion(p),
            max_tokens,
            temperature,
            ResponseFormat::Text,
            ReasoningEffort::None,
            RetryPolicy::none(),
            vec!["\n\n".into()],
        )?;
        let response = self.provider.execute(request, cancel).await?;
        let truncated = response.finish_reason.as_deref() == Some("length");
        Ok(clean_completion(&response.text, p.text_before, p.max_words as usize, truncated))
    }

    /// classify(): asks the classification model about an uncertain context.
    pub async fn classify(
        &self,
        p: &ClassificationPrompt<'_>,
        cancel: &CancellationToken,
    ) -> Result<Option<IntentAssessment>, ProviderError> {
        let request = self.build(
            ModelRole::Classification,
            Feature::IntentClassification,
            RequestType::Classification,
            prompts::classification(p),
            60,
            0.0,
            ResponseFormat::JsonObject,
            ReasoningEffort::None,
            RetryPolicy::background(),
            Vec::new(),
        )?;
        let response = self.provider.execute(request, cancel).await?;
        Ok(parse_ai_classification(&response.text))
    }

    /// Grammar check of one sentence; returns the corrected sentence.
    pub async fn check_grammar(
        &self,
        sentence: &str,
        language: &LanguageProfile,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, ProviderError> {
        let request = self.build(
            ModelRole::Writing,
            Feature::WritingAssistance,
            RequestType::Transform,
            prompts::grammar_check(sentence, language),
            prompts::estimate_tokens(sentence) * 2 + 32,
            0.1,
            ResponseFormat::Text,
            ReasoningEffort::None,
            RetryPolicy::background(),
            Vec::new(),
        )?;
        let response = self.provider.execute(request, cancel).await?;
        let text = prompts::clean_transform_output(&response.text);
        Ok((!text.is_empty()).then_some(text))
    }

    /// transform(): rewrite, enhance, summarize, translate, … the given text.
    pub async fn transform(
        &self,
        p: &TransformPrompt<'_>,
        cancel: &CancellationToken,
    ) -> Result<String, ProviderError> {
        p.action.validate().map_err(|message| ProviderError::BadRequest { status: 0, message })?;
        let role = p.action.role();
        let temperature = match p.action {
            TransformAction::FixSpellingGrammar | TransformAction::Translate { .. } => 0.1,
            TransformAction::ImproveWriting | TransformAction::Clearer | TransformAction::Concise => 0.3,
            TransformAction::Summarize | TransformAction::Explain => 0.3,
            TransformAction::ContinueWriting => 0.7,
            _ => 0.45,
        };
        let request = self.build(
            role,
            p.action.feature(),
            RequestType::Transform,
            prompts::transform(p),
            prompts::transform_max_tokens(p.action, p.text, p.clipboard.map(|(_, c)| c)),
            temperature,
            ResponseFormat::Text,
            if role == ModelRole::Reasoning { ReasoningEffort::Low } else { ReasoningEffort::None },
            RetryPolicy::interactive(),
            Vec::new(),
        )?;
        let response = self.provider.execute(request, cancel).await?;
        let text = prompts::clean_transform_output(&response.text);
        if text.is_empty() {
            return Err(ProviderError::InvalidResponse("empty result".into()));
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::apps::AppCategory;
    use crate::intent::{IntentKind, IntentSource, IntentSubtype};
    use crate::language::detect;
    use crate::prompts::EnhanceStyle;
    use crate::providers::testing::{Script, ScriptedProvider};
    use crate::usage::MemoryUsageSink;

    fn client(scripts: Vec<Script>) -> (Arc<ScriptedProvider>, Arc<MemoryUsageSink>, AiClient) {
        let provider = Arc::new(ScriptedProvider::new(scripts));
        let sink = Arc::new(MemoryUsageSink::default());
        let resilient = Arc::new(ResilientProvider::new(provider.clone(), sink.clone()));
        (provider, sink, AiClient::new(resilient, Routing { configured: true, ..Routing::default() }))
    }

    #[tokio::test(start_paused = true)]
    async fn completion_uses_completion_model_and_cleans_output() {
        let (provider, sink, ai) =
            client(vec![Script::ok("the Redis container was unavailable during startup.", 120, 12)]);
        let language = detect("The deployment failed because");
        let out = ai
            .complete(
                &CompletionPrompt {
                    text_before: "The deployment failed because",
                    kind: IntentKind::Conversation,
                    subtype: None,
                    language: &language,
                    app_name: "Slack",
                    category: AppCategory::Chat,
                    max_words: 12,
                    avoid: &[],
                },
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(out.as_deref(), Some(" the Redis container was unavailable during startup."));
        let call = &provider.calls()[0];
        assert_eq!(call.model, crate::settings::default_models::COMPLETION);
        assert_eq!(call.feature, Feature::InlineCompletion);
        assert_eq!(call.reasoning, ReasoningEffort::None);
        assert!(call.max_output_tokens <= 96);
        assert_eq!(sink.events()[0].feature, Feature::InlineCompletion);
    }

    #[tokio::test(start_paused = true)]
    async fn classification_parses_json() {
        let (provider, _, ai) =
            client(vec![Script::ok(r#"{"kind":"prompt","subtype":"coding","confidence":0.9}"#, 80, 15)]);
        let language = detect("why does this fail");
        let result = ai
            .classify(
                &ClassificationPrompt {
                    app_name: "Chrome",
                    category: AppCategory::Browser,
                    role: None,
                    placeholder: None,
                    language: &language,
                    excerpt: "why does this fail",
                },
                &CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (result.kind, result.subtype, result.source),
            (IntentKind::Prompt, Some(IntentSubtype::Coding), IntentSource::Ai)
        );
        assert_eq!(provider.calls()[0].response_format, ResponseFormat::JsonObject);
    }

    #[tokio::test(start_paused = true)]
    async fn transform_routes_by_action() {
        let (provider, sink, ai) = client(vec![Script::ok(
            "Fix the issue in the following code. Identify the root cause, explain why the error occurs, and provide the smallest correct fix. Avoid unrelated changes.",
            90,
            40,
        )]);
        let language = detect("fix this code it is giving error");
        let action = TransformAction::EnhancePrompt { style: EnhanceStyle::Improve };
        let out = ai
            .transform(
                &TransformPrompt {
                    action: &action,
                    text: "fix this code it is giving error",
                    language: &language,
                    kind: Some(IntentKind::Prompt),
                    subtype: Some(IntentSubtype::Coding),
                    clipboard: None,
                },
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(out.starts_with("Fix the issue"));
        let call = &provider.calls()[0];
        assert_eq!(call.model, crate::settings::default_models::REASONING);
        assert_eq!(call.reasoning, ReasoningEffort::Low);
        assert_eq!(sink.events()[0].feature, Feature::PromptEnhancement);
    }

    #[tokio::test]
    async fn privacy_switch_and_missing_key_block_requests() {
        let (provider, sink, ai) = client(vec![]);
        ai.set_routing(Routing { cloud_enabled: false, configured: true, ..Routing::default() });
        let language = LanguageProfile::english();
        let r = ai.check_grammar("we dont know.", &language, &CancellationToken::new()).await;
        assert_eq!(r, Err(ProviderError::CloudDisabled));
        ai.set_routing(Routing { cloud_enabled: true, configured: false, ..Routing::default() });
        let r = ai.check_grammar("we dont know.", &language, &CancellationToken::new()).await;
        assert_eq!(r, Err(ProviderError::NotConfigured));
        assert!(provider.calls().is_empty());
        assert!(sink.events().is_empty());
        assert!(!ai.automatic_requests_allowed());
    }

    #[tokio::test]
    async fn invalid_transform_parameters_are_rejected_locally() {
        let (provider, _, ai) = client(vec![]);
        let language = LanguageProfile::english();
        let action = TransformAction::Translate { target: String::new() };
        let r = ai
            .transform(
                &TransformPrompt {
                    action: &action,
                    text: "hi",
                    language: &language,
                    kind: None,
                    subtype: None,
                    clipboard: None,
                },
                &CancellationToken::new(),
            )
            .await;
        assert!(matches!(r, Err(ProviderError::BadRequest { .. })));
        assert!(provider.calls().is_empty());
    }
}
