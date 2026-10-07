//! Integration tests for the Groq provider against a local mock server.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use mote_core::providers::resilient::ResilientProvider;
use mote_core::providers::types::*;
use mote_core::providers::{ModelProvider, ProviderError};
use mote_core::usage::{MemoryUsageSink, UsageStatus};
use mote_providers::{ApiKey, GroqProvider};

const KEY: &str = "gsk_test0000000000000000000000000000000000000000000000";

fn provider(server: &MockServer) -> GroqProvider {
    GroqProvider::new(format!("{}/openai/v1", server.uri()), Some(ApiKey::new(KEY))).unwrap()
}

fn request(model: &str) -> GenerationRequest {
    GenerationRequest {
        model: model.into(),
        fallback_models: vec![],
        messages: vec![ChatMessage::system("Continue."), ChatMessage::user("The deployment failed because")],
        max_output_tokens: 32,
        temperature: 0.3,
        stop: vec![],
        response_format: ResponseFormat::Text,
        reasoning: ReasoningEffort::None,
        timeout: Duration::from_secs(5),
        feature: Feature::InlineCompletion,
        request_type: RequestType::Completion,
        retry: RetryPolicy::none(),
    }
}

/// A response shaped like Groq's (captured from the live API, content changed).
fn completion_body(content: &str) -> serde_json::Value {
    json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1_791_000_000,
        "model": "qwen/qwen3.8-27b",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
        "usage": {"queue_time": 0.048, "prompt_tokens": 43, "prompt_time": 0.0034, "completion_tokens": 10,
                  "completion_time": 0.0198, "total_tokens": 53, "total_time": 0.0232},
        "usage_breakdown": null,
        "system_fingerprint": "fp_x",
        "x_groq": {"id": "req_abc"},
        "service_tier": "on_demand"
    })
}

fn rate_limit_headers(template: ResponseTemplate) -> ResponseTemplate {
    template
        .insert_header("x-ratelimit-limit-requests", "1000")
        .insert_header("x-ratelimit-remaining-requests", "999")
        .insert_header("x-ratelimit-reset-requests", "1m26.4s")
        .insert_header("x-ratelimit-limit-tokens", "8000")
        .insert_header("x-ratelimit-remaining-tokens", "7761")
        .insert_header("x-ratelimit-reset-tokens", "1.792s")
}

#[tokio::test]
async fn successful_completion_extracts_text_usage_latency_and_limits() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openai/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .and(body_partial_json(
            json!({"model": "qwen/qwen3.8-27b", "reasoning_effort": "none", "max_completion_tokens": 32}),
        ))
        .respond_with(rate_limit_headers(
            ResponseTemplate::new(200).set_body_json(completion_body("the cache was cold.")),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let p = provider(&server);
    let r = p.generate(&request("qwen/qwen3.8-27b"), &CancellationToken::new()).await.unwrap();
    assert_eq!(r.text, "the cache was cold.");
    assert_eq!(r.model, "qwen/qwen3.8-27b");
    assert_eq!(r.finish_reason.as_deref(), Some("stop"));
    let usage = r.usage.unwrap();
    assert_eq!((usage.input_tokens, usage.output_tokens, usage.total_tokens), (43, 10, 53));
    assert_eq!(r.request_id.as_deref(), Some("req_abc"));
    assert!(r.provider_latency.unwrap() < Duration::from_millis(30));
    let limits = r.rate_limit.unwrap();
    assert_eq!((limits.requests_limit, limits.tokens_remaining), (Some(1000), Some(7761)));
    assert_eq!(p.usage_snapshot().unwrap().tokens_limit, Some(8000), "getUsage() exposes provider limits");
}

#[tokio::test]
async fn reasoning_tokens_are_reported() {
    let server = MockServer::start().await;
    let mut body = completion_body("ok");
    body["usage"]["completion_tokens_details"] = json!({"reasoning_tokens": 13});
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({"reasoning_effort": "low", "include_reasoning": false, "max_completion_tokens": 288}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let r = provider(&server).generate(&request("openai/gpt-oss-20b"), &CancellationToken::new()).await.unwrap();
    assert_eq!(r.usage.unwrap().reasoning_tokens, Some(13));
}

#[tokio::test]
async fn invalid_key_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            json!({"error": {"message": "Invalid API Key", "type": "invalid_request_error", "code": "invalid_api_key"}}),
        ))
        .mount(&server)
        .await;
    let r = provider(&server).generate(&request("qwen/qwen3.8-27b"), &CancellationToken::new()).await;
    assert_eq!(r.unwrap_err(), ProviderError::Unauthorized);
}

#[tokio::test]
async fn unavailable_model_is_detected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": {
            "message": "The model `llama-3.1-8b-instant` does not exist or you do not have access to it.",
            "type": "invalid_request_error", "code": "model_not_found"}})))
        .mount(&server)
        .await;
    let r = provider(&server).generate(&request("llama-3.1-8b-instant"), &CancellationToken::new()).await;
    assert_eq!(r.unwrap_err(), ProviderError::ModelUnavailable { model: "llama-3.1-8b-instant".into() });
}

#[tokio::test]
async fn decommissioned_model_is_unavailable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": {
            "message": "The model has been decommissioned", "type": "invalid_request_error", "code": "model_decommissioned"}})))
        .mount(&server)
        .await;
    let r = provider(&server).generate(&request("old-model"), &CancellationToken::new()).await;
    assert!(matches!(r, Err(ProviderError::ModelUnavailable { .. })));
}

#[tokio::test]
async fn rate_limit_carries_retry_after_and_snapshot() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(rate_limit_headers(ResponseTemplate::new(429).insert_header("retry-after", "7").set_body_json(
            json!({"error": {"message": "Rate limit reached", "type": "requests", "code": "rate_limit_exceeded"}}),
        )))
        .mount(&server)
        .await;
    let r = provider(&server).generate(&request("qwen/qwen3.8-27b"), &CancellationToken::new()).await;
    match r {
        Err(ProviderError::RateLimited { retry_after, snapshot }) => {
            assert_eq!(retry_after, Some(Duration::from_secs(7)));
            assert_eq!(snapshot.unwrap().requests_remaining, Some(999));
        }
        other => panic!("expected rate limit, got {other:?}"),
    }
}

#[tokio::test]
async fn server_errors_are_transient() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
        .mount(&server)
        .await;
    let r = provider(&server).generate(&request("qwen/qwen3.8-27b"), &CancellationToken::new()).await;
    let error = r.unwrap_err();
    assert!(matches!(error, ProviderError::Server { status: 503, .. }));
    assert!(error.is_transient());
}

#[tokio::test]
async fn slow_responses_time_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(completion_body("late")).set_delay(Duration::from_secs(3)),
        )
        .mount(&server)
        .await;
    let mut req = request("qwen/qwen3.8-27b");
    req.timeout = Duration::from_millis(300);
    let r = provider(&server).generate(&req, &CancellationToken::new()).await;
    assert_eq!(r.unwrap_err(), ProviderError::Timeout);
}

#[tokio::test]
async fn unreachable_server_is_a_network_error() {
    // Port 9 (discard) on localhost is closed on CI machines and developer laptops.
    let p = GroqProvider::new("http://127.0.0.1:9/openai/v1", Some(ApiKey::new(KEY))).unwrap();
    let r = p.generate(&request("qwen/qwen3.8-27b"), &CancellationToken::new()).await;
    assert!(matches!(r, Err(ProviderError::Network(_))), "{r:?}");
}

#[tokio::test]
async fn cancellation_aborts_the_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(completion_body("late")).set_delay(Duration::from_secs(5)),
        )
        .mount(&server)
        .await;
    let p = provider(&server);
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        c.cancel();
    });
    let started = std::time::Instant::now();
    let r = p.generate(&request("qwen/qwen3.8-27b"), &cancel).await;
    assert_eq!(r.unwrap_err(), ProviderError::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn missing_key_never_sends_a_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&server).await;
    let p = GroqProvider::new(format!("{}/openai/v1", server.uri()), None).unwrap();
    let r = p.generate(&request("qwen/qwen3.8-27b"), &CancellationToken::new()).await;
    assert_eq!(r.unwrap_err(), ProviderError::NotConfigured);
}

#[tokio::test]
async fn models_are_listed_and_non_chat_models_flagged() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/openai/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"object": "list", "data": [
            {"id": "qwen/qwen3.8-27b", "owned_by": "Alibaba Cloud", "active": true, "context_window": 131072, "max_completion_tokens": 16384},
            {"id": "openai/gpt-oss-20b", "owned_by": "OpenAI", "active": true, "context_window": 131072, "max_completion_tokens": 65536},
            {"id": "whisper-large-v3", "owned_by": "OpenAI", "active": true, "context_window": 448},
            {"id": "retired-model", "owned_by": "X", "active": false}
        ]})))
        .mount(&server)
        .await;
    let models = provider(&server).list_models().await.unwrap();
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["openai/gpt-oss-20b", "qwen/qwen3.8-27b", "whisper-large-v3"]);
    assert!(!models.iter().find(|m| m.id == "whisper-large-v3").unwrap().supports_chat);
    assert_eq!(models[1].max_output_tokens, Some(16384));
}

#[tokio::test]
async fn health_check_reports_missing_models_and_bad_keys() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/openai/v1/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": [{"id": "openai/gpt-oss-20b", "active": true}]})),
        )
        .mount(&server)
        .await;
    let report = provider(&server).health_check(&["openai/gpt-oss-20b".into(), "qwen/qwen3.8-27b".into()]).await;
    assert!(!report.ok);
    assert_eq!(report.missing_models, vec!["qwen/qwen3.8-27b".to_string()]);
    assert!(report.message.unwrap().contains("qwen/qwen3.8-27b"));

    let bad = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": {"message": "Invalid API Key"}})))
        .mount(&bad)
        .await;
    let report =
        GroqProvider::verify_key(&format!("{}/openai/v1", bad.uri()), ApiKey::new("gsk_wrong_key_value_123456"), &[])
            .await;
    assert!(!report.ok);
    assert!(report.message.unwrap().contains("rejected"));
}

#[tokio::test]
async fn api_key_never_appears_in_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"error": {"message": format!("bad request for key {KEY}")}})),
        )
        .mount(&server)
        .await;
    let error = provider(&server).generate(&request("qwen/qwen3.8-27b"), &CancellationToken::new()).await.unwrap_err();
    assert!(!format!("{error:?}").contains(KEY));
    assert!(!error.user_message().contains(KEY));
}

/// The full stack: resilient wrapper + Groq provider + usage accounting.
#[tokio::test]
async fn resilient_stack_retries_server_errors_and_meters_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(completion_body("recovered")))
        .with_priority(2)
        .mount(&server)
        .await;
    let sink = Arc::new(MemoryUsageSink::default());
    let resilient = ResilientProvider::new(Arc::new(provider(&server)), sink.clone());
    let mut req = request("qwen/qwen3.8-27b");
    req.retry = RetryPolicy::interactive();
    let r = resilient.execute(req, &CancellationToken::new()).await.unwrap();
    assert_eq!(r.text, "recovered");
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].status, events[0].attempts, events[0].total_tokens), (UsageStatus::Success, 2, Some(53)));
    assert_eq!(events[0].provider, "groq");
}
