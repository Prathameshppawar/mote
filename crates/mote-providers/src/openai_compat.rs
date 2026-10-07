//! OpenAI-compatible chat-completions client.
//!
//! Groq exposes an OpenAI-compatible API, so [`crate::GroqProvider`] is this
//! client with the [`Dialect::Groq`] adjustments: per-model reasoning
//! parameters, `max_completion_tokens`, Groq's usage timings and rate-limit
//! headers. The [`Dialect::Generic`] variant targets other compatible servers
//! (including local ones such as Ollama or LM Studio) for future providers.

use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio_util::sync::CancellationToken;

use mote_core::providers::types::*;
use mote_core::providers::{ModelProvider, ProviderError};

use crate::ratelimit::{parse_rate_limits, parse_retry_after};
use crate::secret::ApiKey;

/// Server-specific behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Groq,
    Generic,
}

/// How a model family controls hidden reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningControl {
    /// No reasoning parameters.
    None,
    /// GPT-OSS: `reasoning_effort` low/medium/high (cannot be disabled),
    /// `include_reasoning: false`; reasoning tokens count toward the output budget.
    GptOss,
    /// Qwen 3.x: `reasoning_effort` none/default/low/medium/high and
    /// `reasoning_format: hidden`.
    Qwen { supports_levels: bool },
}

impl ReasoningControl {
    pub fn for_model(dialect: Dialect, model: &str) -> Self {
        if dialect == Dialect::Generic {
            return Self::None;
        }
        let m = model.to_lowercase();
        if m.starts_with("openai/gpt-oss") {
            Self::GptOss
        } else if m.starts_with("qwen/qwen3") {
            Self::Qwen { supports_levels: m != "qwen/qwen3-32b" }
        } else {
            Self::None
        }
    }

    /// Extra output tokens to reserve for hidden reasoning.
    fn headroom(self, effort: ReasoningEffort) -> u32 {
        match (self, effort) {
            (Self::GptOss, ReasoningEffort::None | ReasoningEffort::Low) => 256,
            (Self::GptOss, ReasoningEffort::Medium) => 1_024,
            (Self::GptOss, ReasoningEffort::High) => 4_096,
            (Self::Qwen { .. }, ReasoningEffort::None) => 0,
            (Self::Qwen { .. }, ReasoningEffort::Low) => 512,
            (Self::Qwen { .. }, ReasoningEffort::Medium) => 1_536,
            (Self::Qwen { .. }, ReasoningEffort::High) => 4_096,
            (Self::None, _) => 0,
        }
    }
}

/// Connection settings.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub provider_id: &'static str,
    pub display_name: &'static str,
    /// Base URL, e.g. `https://api.groq.com/openai/v1`.
    pub base_url: String,
    pub dialect: Dialect,
}

/// An OpenAI-compatible provider.
pub struct OpenAiCompatibleProvider {
    http: reqwest::Client,
    config: RwLock<ClientConfig>,
    api_key: Arc<RwLock<Option<ApiKey>>>,
    last_rate_limit: Mutex<Option<RateLimitSnapshot>>,
}

const USER_AGENT_VALUE: &str =
    concat!("Mote/", env!("CARGO_PKG_VERSION"), " (+https://github.com/Prathameshppawar/mote)");

impl OpenAiCompatibleProvider {
    pub fn new(config: ClientConfig, api_key: Option<ApiKey>) -> Result<Self, ProviderError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_nodelay(true)
            .build()
            .map_err(|e| ProviderError::Network(format!("could not create HTTP client: {e}")))?;
        Ok(Self {
            http,
            config: RwLock::new(config),
            api_key: Arc::new(RwLock::new(api_key)),
            last_rate_limit: Mutex::new(None),
        })
    }

    fn config(&self) -> ClientConfig {
        self.config.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// Replaces the API key (or removes it with `None`).
    pub fn set_api_key(&self, key: Option<ApiKey>) {
        *self.api_key.write().unwrap_or_else(std::sync::PoisonError::into_inner) = key;
    }

    pub fn has_api_key(&self) -> bool {
        self.api_key.read().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().is_some_and(|k| !k.is_empty())
    }

    pub fn set_base_url(&self, base_url: String) {
        self.config.write().unwrap_or_else(std::sync::PoisonError::into_inner).base_url = base_url;
    }

    fn authorization(&self) -> Result<String, ProviderError> {
        let guard = self.api_key.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        match guard.as_ref() {
            Some(key) if !key.is_empty() => Ok(format!("Bearer {}", key.expose())),
            _ => Err(ProviderError::NotConfigured),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.config().base_url.trim_end_matches('/'), path)
    }

    /// Builds the JSON body for a chat completion.
    pub fn request_body(dialect: Dialect, request: &GenerationRequest) -> Value {
        let control = ReasoningControl::for_model(dialect, &request.model);
        let mut body = Map::new();
        body.insert("model".into(), json!(request.model));
        body.insert(
            "messages".into(),
            Value::Array(
                request
                    .messages
                    .iter()
                    .map(|m| {
                        let role = match m.role {
                            ChatRole::System => "system",
                            ChatRole::User => "user",
                            ChatRole::Assistant => "assistant",
                        };
                        json!({ "role": role, "content": m.content })
                    })
                    .collect(),
            ),
        );
        let max_tokens = request.max_output_tokens.saturating_add(control.headroom(request.reasoning));
        match dialect {
            Dialect::Groq => body.insert("max_completion_tokens".into(), json!(max_tokens)),
            Dialect::Generic => body.insert("max_tokens".into(), json!(max_tokens)),
        };
        body.insert("temperature".into(), json!(request.temperature));
        body.insert("stream".into(), json!(false));
        if !request.stop.is_empty() {
            body.insert("stop".into(), json!(request.stop.iter().take(4).collect::<Vec<_>>()));
        }
        if request.response_format == ResponseFormat::JsonObject {
            body.insert("response_format".into(), json!({ "type": "json_object" }));
        }
        match control {
            ReasoningControl::GptOss => {
                let effort = match request.reasoning {
                    ReasoningEffort::None | ReasoningEffort::Low => "low",
                    ReasoningEffort::Medium => "medium",
                    ReasoningEffort::High => "high",
                };
                body.insert("reasoning_effort".into(), json!(effort));
                body.insert("include_reasoning".into(), json!(false));
            }
            ReasoningControl::Qwen { supports_levels } => {
                let effort = match (request.reasoning, supports_levels) {
                    (ReasoningEffort::None, _) => "none",
                    (_, false) => "default",
                    (ReasoningEffort::Low, true) => "low",
                    (ReasoningEffort::Medium, true) => "medium",
                    (ReasoningEffort::High, true) => "high",
                };
                body.insert("reasoning_effort".into(), json!(effort));
                if effort != "none" {
                    body.insert("reasoning_format".into(), json!("hidden"));
                }
            }
            ReasoningControl::None => {}
        }
        Value::Object(body)
    }

    fn remember_rate_limit(&self, headers: &HeaderMap) -> Option<RateLimitSnapshot> {
        let snapshot = parse_rate_limits(headers, Utc::now())?;
        *self.last_rate_limit.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snapshot.clone());
        Some(snapshot)
    }

    async fn send_chat(&self, request: &GenerationRequest) -> Result<GenerationResponse, ProviderError> {
        let config = self.config();
        let auth = self.authorization()?;
        let body = Self::request_body(config.dialect, request);
        let started = Instant::now();
        let response = self
            .http
            .post(self.url("chat/completions"))
            .timeout(request.timeout)
            .header(AUTHORIZATION, auth)
            .header(CONTENT_TYPE, "application/json")
            .header(USER_AGENT, USER_AGENT_VALUE)
            .json(&body)
            .send()
            .await
            .map_err(map_transport_error)?;
        let status = response.status();
        let headers = response.headers().clone();
        let snapshot = self.remember_rate_limit(&headers);
        let bytes = response.bytes().await.map_err(map_transport_error)?;
        if !status.is_success() {
            return Err(map_http_error(status, &headers, &bytes, &request.model, snapshot));
        }
        let parsed: ChatCompletionResponse = serde_json::from_slice(&bytes)
            .map_err(|e| ProviderError::InvalidResponse(format!("could not parse completion: {e}")))?;
        let choice =
            parsed.choices.into_iter().next().ok_or_else(|| ProviderError::InvalidResponse("no choices".into()))?;
        let text = strip_think_blocks(choice.message.content.as_deref().unwrap_or_default());
        let usage = parsed.usage.as_ref().map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            total_tokens: if u.total_tokens > 0 { u.total_tokens } else { u.prompt_tokens + u.completion_tokens },
            reasoning_tokens: u.completion_tokens_details.as_ref().and_then(|d| d.reasoning_tokens),
        });
        let provider_latency = parsed
            .usage
            .as_ref()
            .and_then(|u| u.total_time)
            .filter(|t| t.is_finite() && *t >= 0.0)
            .map(Duration::from_secs_f64);
        Ok(GenerationResponse {
            text,
            model: parsed.model.unwrap_or_else(|| request.model.clone()),
            finish_reason: choice.finish_reason,
            usage,
            latency: started.elapsed(),
            provider_latency,
            request_id: parsed.x_groq.and_then(|x| x.id).or(parsed.id),
            rate_limit: snapshot,
        })
    }

    async fn fetch_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let auth = self.authorization()?;
        let response = self
            .http
            .get(self.url("models"))
            .timeout(Duration::from_secs(15))
            .header(AUTHORIZATION, auth)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .send()
            .await
            .map_err(map_transport_error)?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.bytes().await.map_err(map_transport_error)?;
        if !status.is_success() {
            return Err(map_http_error(status, &headers, &bytes, "", None));
        }
        let parsed: ModelsResponse = serde_json::from_slice(&bytes)
            .map_err(|e| ProviderError::InvalidResponse(format!("could not parse models: {e}")))?;
        let mut models: Vec<ModelInfo> = parsed
            .data
            .into_iter()
            .filter(|m| m.active.unwrap_or(true))
            .map(|m| {
                let supports_chat = is_chat_model(&m.id);
                ModelInfo {
                    id: m.id,
                    owned_by: m.owned_by,
                    context_window: m.context_window,
                    max_output_tokens: m.max_completion_tokens,
                    supports_chat,
                }
            })
            .collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }
}

/// Whether a model ID names a text chat model (not speech, TTS or a guard model).
pub fn is_chat_model(id: &str) -> bool {
    let id = id.to_lowercase();
    !["whisper", "tts", "orpheus", "playai", "guard", "safeguard", "distil-whisper", "embed"]
        .iter()
        .any(|m| id.contains(m))
}

/// Removes `<think>…</think>` blocks some reasoning models emit inline.
pub fn strip_think_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim_start_matches('\n').to_string()
}

fn map_transport_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else if error.is_connect() {
        ProviderError::Network("could not connect".into())
    } else if error.is_request() || error.is_body() || error.is_decode() {
        ProviderError::Network("connection interrupted".into())
    } else {
        ProviderError::Network("request failed".into())
    }
}

#[derive(Debug, Deserialize, Default)]
struct ErrorEnvelope {
    error: Option<ErrorBody>,
}

#[derive(Debug, Deserialize, Default)]
struct ErrorBody {
    message: Option<String>,
    code: Option<Value>,
}

fn map_http_error(
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
    model: &str,
    snapshot: Option<RateLimitSnapshot>,
) -> ProviderError {
    let envelope: ErrorEnvelope = serde_json::from_slice(body).unwrap_or_default();
    let (message, code) = envelope
        .error
        .map(|e| {
            (
                e.message.unwrap_or_default(),
                e.code.map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| c.to_string())),
            )
        })
        .unwrap_or_default();
    let message = mote_core::privacy::redact::redact_secrets(&message.chars().take(300).collect::<String>());
    let code = code.unwrap_or_default();
    match status.as_u16() {
        401 => ProviderError::Unauthorized,
        404 if code == "model_not_found" || message.contains("does not exist") => {
            ProviderError::ModelUnavailable { model: model.to_string() }
        }
        400 | 404 if code == "model_decommissioned" || code == "model_not_active" => {
            ProviderError::ModelUnavailable { model: model.to_string() }
        }
        429 => ProviderError::RateLimited { retry_after: parse_retry_after(headers), snapshot: snapshot.map(Box::new) },
        498 | 500 | 502 | 503 | 504 => ProviderError::Server { status: status.as_u16(), message },
        s if s >= 500 => ProviderError::Server { status: s, message },
        s => ProviderError::BadRequest {
            status: s,
            message: if message.is_empty() { format!("HTTP {s}") } else { message },
        },
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    id: Option<String>,
    model: Option<String>,
    choices: Vec<Choice>,
    usage: Option<Usage>,
    x_groq: Option<XGroq>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
    total_time: Option<f64>,
    completion_tokens_details: Option<CompletionDetails>,
}

#[derive(Debug, Deserialize)]
struct CompletionDetails {
    reasoning_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct XGroq {
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    owned_by: Option<String>,
    active: Option<bool>,
    context_window: Option<u32>,
    max_completion_tokens: Option<u32>,
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        let config = self.config();
        ProviderDescriptor { id: config.provider_id, display_name: config.display_name }
    }

    async fn generate(
        &self,
        request: &GenerationRequest,
        cancel: &CancellationToken,
    ) -> Result<GenerationResponse, ProviderError> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ProviderError::Cancelled),
            result = self.send_chat(request) => result,
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.fetch_models().await
    }

    fn usage_snapshot(&self) -> Option<RateLimitSnapshot> {
        self.last_rate_limit.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(model: &str, reasoning: ReasoningEffort) -> GenerationRequest {
        GenerationRequest {
            model: model.into(),
            fallback_models: vec![],
            messages: vec![ChatMessage::system("sys"), ChatMessage::user("hi")],
            max_output_tokens: 32,
            temperature: 0.3,
            stop: vec!["\n\n".into()],
            response_format: ResponseFormat::Text,
            reasoning,
            timeout: Duration::from_secs(5),
            feature: Feature::InlineCompletion,
            request_type: RequestType::Completion,
            retry: RetryPolicy::none(),
        }
    }

    #[test]
    fn qwen_disables_reasoning() {
        let body =
            OpenAiCompatibleProvider::request_body(Dialect::Groq, &request("qwen/qwen3.8-27b", ReasoningEffort::None));
        assert_eq!(body["reasoning_effort"], "none");
        assert!(body.get("reasoning_format").is_none());
        assert_eq!(body["max_completion_tokens"], 32);
        assert_eq!(body["stop"][0], "\n\n");
        assert_eq!(body["messages"][0]["role"], "system");
    }

    #[test]
    fn gpt_oss_gets_reasoning_headroom() {
        let body = OpenAiCompatibleProvider::request_body(
            Dialect::Groq,
            &request("openai/gpt-oss-20b", ReasoningEffort::None),
        );
        assert_eq!(body["reasoning_effort"], "low", "GPT-OSS cannot disable reasoning");
        assert_eq!(body["include_reasoning"], false);
        assert_eq!(body["max_completion_tokens"], 32 + 256);
        let high = OpenAiCompatibleProvider::request_body(
            Dialect::Groq,
            &request("openai/gpt-oss-120b", ReasoningEffort::High),
        );
        assert_eq!(high["reasoning_effort"], "high");
    }

    #[test]
    fn qwen_reasoning_levels() {
        let body =
            OpenAiCompatibleProvider::request_body(Dialect::Groq, &request("qwen/qwen3.8-27b", ReasoningEffort::Low));
        assert_eq!(body["reasoning_effort"], "low");
        assert_eq!(body["reasoning_format"], "hidden");
        let old =
            OpenAiCompatibleProvider::request_body(Dialect::Groq, &request("qwen/qwen3-32b", ReasoningEffort::Medium));
        assert_eq!(old["reasoning_effort"], "default");
    }

    #[test]
    fn generic_dialect_sends_plain_openai_requests() {
        let mut r = request("llama3.2", ReasoningEffort::High);
        r.response_format = ResponseFormat::JsonObject;
        let body = OpenAiCompatibleProvider::request_body(Dialect::Generic, &r);
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("max_completion_tokens").is_none());
        assert_eq!(body["max_tokens"], 32);
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[test]
    fn unknown_models_get_no_reasoning_parameters() {
        let body = OpenAiCompatibleProvider::request_body(
            Dialect::Groq,
            &request("llama-3.1-8b-instant", ReasoningEffort::Low),
        );
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn think_blocks_are_removed() {
        assert_eq!(strip_think_blocks("<think>secret plan</think>\nAnswer"), "Answer");
        assert_eq!(strip_think_blocks("a<think>x</think>b<think>y</think>c"), "abc");
        assert_eq!(strip_think_blocks("<think>unterminated"), "");
        assert_eq!(strip_think_blocks("plain"), "plain");
    }

    #[test]
    fn chat_model_filter() {
        assert!(is_chat_model("openai/gpt-oss-20b"));
        assert!(is_chat_model("qwen/qwen3.8-27b"));
        assert!(!is_chat_model("whisper-large-v3"));
        assert!(!is_chat_model("meta-llama/llama-prompt-guard-2-86m"));
        assert!(!is_chat_model("openai/gpt-oss-safeguard-20b"));
        assert!(!is_chat_model("canopylabs/orpheus-v1-english"));
    }
}
