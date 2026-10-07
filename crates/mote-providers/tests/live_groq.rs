//! Live tests against the real Groq API.
//!
//! Ignored by default. Run with a key in `MOTE_GROQ_API_KEY` (or in
//! `~/.config/mote-dev/groq-api-key`):
//!
//! ```sh
//! cargo test -p mote-providers --test live_groq -- --ignored --test-threads=1
//! ```
//!
//! Each test makes at most a couple of tiny requests.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use mote_core::ai::{AiClient, Routing};
use mote_core::intent::apps::AppCategory;
use mote_core::intent::{IntentKind, IntentSubtype};
use mote_core::language::detect;
use mote_core::prompts::{ClassificationPrompt, CompletionPrompt, EnhanceStyle, TransformAction, TransformPrompt};
use mote_core::providers::resilient::ResilientProvider;
use mote_core::providers::types::*;
use mote_core::providers::{ModelProvider, ProviderError};
use mote_core::settings::{default_models, ModelAssignments, GROQ_BASE_URL};
use mote_core::usage::MemoryUsageSink;
use mote_providers::{ApiKey, GroqProvider};

fn live_key() -> Option<ApiKey> {
    if let Ok(key) = std::env::var("MOTE_GROQ_API_KEY") {
        return Some(ApiKey::new(key));
    }
    let home = std::env::var_os("HOME")?;
    let path = std::path::Path::new(&home).join(".config/mote-dev/groq-api-key");
    std::fs::read_to_string(path).ok().map(ApiKey::new)
}

fn live_provider() -> GroqProvider {
    let key = live_key().expect("set MOTE_GROQ_API_KEY or ~/.config/mote-dev/groq-api-key");
    GroqProvider::new(GROQ_BASE_URL, Some(key)).unwrap()
}

fn ai(provider: GroqProvider) -> (AiClient, Arc<MemoryUsageSink>) {
    let sink = Arc::new(MemoryUsageSink::default());
    let resilient = Arc::new(ResilientProvider::new(Arc::new(provider), sink.clone()));
    let routing = Routing {
        models: ModelAssignments::default(),
        timeout: Duration::from_secs(30),
        cloud_enabled: true,
        configured: true,
    };
    (AiClient::new(resilient, routing), sink)
}

#[tokio::test]
#[ignore = "live Groq API"]
async fn live_default_models_are_available() {
    let models = live_provider().list_models().await.unwrap();
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    for model in [default_models::COMPLETION, default_models::REASONING, default_models::FALLBACK] {
        assert!(ids.contains(&model), "{model} missing from {ids:?}");
    }
}

#[tokio::test]
#[ignore = "live Groq API"]
async fn live_inline_completion() {
    let (ai, sink) = ai(live_provider());
    let language = detect("The deployment failed because");
    let result = ai
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
    let text = result.expect("a continuation");
    eprintln!("completion: {text:?}");
    assert!(text.starts_with(' '), "joins with a space after a complete word");
    assert!(text.split_whitespace().count() <= 12);
    let event = &sink.events()[0];
    assert!(event.total_tokens.unwrap() > 0);
    assert!(event.latency_ms.unwrap() > 0);
}

#[tokio::test]
#[ignore = "live Groq API"]
async fn live_marathi_completion_stays_in_latin_script() {
    let (ai, _) = ai(live_provider());
    let text_before = "udya client la call karaycha aahe, mhanun";
    let language = detect(text_before);
    let text = ai
        .complete(
            &CompletionPrompt {
                text_before,
                kind: IntentKind::Conversation,
                subtype: None,
                language: &language,
                app_name: "WhatsApp",
                category: AppCategory::Chat,
                max_words: 10,
                avoid: &[],
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .expect("a continuation");
    eprintln!("marathi completion: {text:?}");
    assert!(!text.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c)), "no Devanagari");
}

#[tokio::test]
#[ignore = "live Groq API"]
async fn live_json_classification() {
    let (ai, _) = ai(live_provider());
    let language = detect("can you summarize the attached quarterly report and list the risks");
    let assessment = ai
        .classify(
            &ClassificationPrompt {
                app_name: "Google Chrome",
                category: AppCategory::Browser,
                role: None,
                placeholder: Some("Ask anything"),
                language: &language,
                excerpt: "can you summarize the attached quarterly report and list the risks",
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .expect("valid JSON");
    eprintln!("classification: {assessment:?}");
    assert_eq!(assessment.kind, IntentKind::Prompt);
}

#[tokio::test]
#[ignore = "live Groq API"]
async fn live_prompt_enhancement_rewrites_without_answering() {
    let (ai, _) = ai(live_provider());
    let cases = [
        ("write python script that reads sales csv and plots monthly totals", Some(IntentSubtype::Coding)),
        ("mujhe manager ko 3 din ki leave ke liye email likhna hai, reason family function hai", None),
    ];
    for (draft, subtype) in cases {
        let language = detect(draft);
        let action = TransformAction::EnhancePrompt { style: EnhanceStyle::Improve };
        let prompt = TransformPrompt {
            action: &action,
            text: draft,
            language: &language,
            kind: Some(IntentKind::Prompt),
            subtype,
            clipboard: None,
        };
        let result = ai.transform(&prompt, &CancellationToken::new()).await.unwrap();
        eprintln!("enhanced: {:?}", result.text);
        assert!(!result.truncated);
        assert_ne!(result.text.trim(), draft);
        let lower = result.text.to_lowercase();
        assert!(!lower.contains("import ") && !lower.contains("```"), "answered instead of rewriting: {lower}");
        assert!(!result.text.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c)), "kept the Latin script");
        assert!(result.text.chars().count() < 2_000, "stays a prompt, not an essay");
    }
}

#[tokio::test]
#[ignore = "live Groq API"]
async fn live_invalid_key_is_rejected() {
    let p =
        GroqProvider::new(GROQ_BASE_URL, Some(ApiKey::new("gsk_this_key_is_not_valid_0000000000000000000000000000")))
            .unwrap();
    assert_eq!(p.list_models().await.unwrap_err(), ProviderError::Unauthorized);
}

#[tokio::test]
#[ignore = "live Groq API"]
async fn live_unknown_model_is_unavailable() {
    let request = GenerationRequest {
        model: "mote-does-not-exist".into(),
        fallback_models: vec![],
        messages: vec![ChatMessage::user("hi")],
        max_output_tokens: 4,
        temperature: 0.0,
        stop: vec![],
        response_format: ResponseFormat::Text,
        reasoning: ReasoningEffort::None,
        timeout: Duration::from_secs(20),
        feature: Feature::InlineCompletion,
        request_type: RequestType::Completion,
        retry: RetryPolicy::none(),
    };
    let r = live_provider().generate(&request, &CancellationToken::new()).await;
    assert!(matches!(r, Err(ProviderError::ModelUnavailable { .. })), "{r:?}");
}
