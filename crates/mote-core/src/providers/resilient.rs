//! Retries, model fallback, rate-limit backoff, offline detection and usage
//! accounting around any [`ModelProvider`].
//!
//! **Accounting invariant:** every logical request that reaches the provider
//! produces exactly one [`UsageEvent`], however many HTTP attempts it took.
//! Tokens come from the final successful attempt; `attempts` and
//! `rate_limit_hits` record what happened on the way. Requests that never
//! leave the machine (no key, cloud disabled, local backoff, rejected key)
//! record nothing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::{GenerationRequest, GenerationResponse, ModelProvider, ProviderError};
use crate::providers::types::Feature;
use crate::usage::{UsageEvent, UsageSink, UsageStatus};

/// How long a model that returned "unavailable" is skipped.
const UNAVAILABLE_TTL: Duration = Duration::from_secs(10 * 60);
/// Consecutive network failures after which Mote considers itself offline.
const OFFLINE_THRESHOLD: u32 = 2;
/// While offline, automatic requests are skipped for this long after a failure.
const OFFLINE_BACKOFF: Duration = Duration::from_secs(30);
/// Fallback wait when a 429 carries no `retry-after`.
const DEFAULT_RATE_LIMIT_WAIT: Duration = Duration::from_secs(5);

/// The latest request outcome, for diagnostics. Contains no content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RequestOutcome {
    pub feature: Feature,
    pub model: String,
    pub status: UsageStatus,
    pub error_kind: Option<String>,
    pub latency_ms: u32,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub at: DateTime<Utc>,
    pub message: Option<String>,
}

#[derive(Default)]
struct State {
    unavailable_models: HashMap<String, Instant>,
    rate_limited_until: Option<Instant>,
    /// The provider rejected the API key; cleared by a success or a new key.
    unauthorized: bool,
    network_failures: u32,
    last_network_failure: Option<Instant>,
    last_outcome: Option<RequestOutcome>,
}

/// Wraps a provider with Mote's resilience and metering policy.
pub struct ResilientProvider {
    inner: Arc<dyn ModelProvider>,
    usage: Arc<dyn UsageSink>,
    state: Mutex<State>,
}

impl ResilientProvider {
    pub fn new(inner: Arc<dyn ModelProvider>, usage: Arc<dyn UsageSink>) -> Self {
        Self { inner, usage, state: Mutex::new(State::default()) }
    }

    pub fn inner(&self) -> &Arc<dyn ModelProvider> {
        &self.inner
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Until when automatic requests should wait because of a rate limit.
    pub fn rate_limited_until(&self) -> Option<Instant> {
        self.state().rate_limited_until.filter(|until| *until > Instant::now())
    }

    /// Whether the provider rejected the API key. Automatic requests stop
    /// until the key changes or a user-initiated request succeeds.
    pub fn is_unauthorized(&self) -> bool {
        self.state().unauthorized
    }

    /// Whether recent network failures indicate the provider is unreachable.
    pub fn is_offline(&self) -> bool {
        let state = self.state();
        state.network_failures >= OFFLINE_THRESHOLD
            && state.last_network_failure.is_some_and(|t| t.elapsed() < OFFLINE_BACKOFF)
    }

    /// Models currently skipped because the provider reported them unavailable.
    pub fn unavailable_models(&self) -> Vec<String> {
        let state = self.state();
        let mut models: Vec<String> = state
            .unavailable_models
            .iter()
            .filter(|(_, at)| at.elapsed() < UNAVAILABLE_TTL)
            .map(|(m, _)| m.clone())
            .collect();
        models.sort();
        models
    }

    pub fn last_outcome(&self) -> Option<RequestOutcome> {
        self.state().last_outcome.clone()
    }

    /// Forgets transient state (after the API key or models change).
    pub fn reset(&self) {
        let mut state = self.state();
        state.unavailable_models.clear();
        state.rate_limited_until = None;
        state.unauthorized = false;
        state.network_failures = 0;
        state.last_network_failure = None;
    }

    fn model_chain(&self, request: &GenerationRequest) -> Vec<String> {
        let mut chain: Vec<String> = Vec::new();
        for model in std::iter::once(&request.model).chain(request.fallback_models.iter()) {
            if !model.is_empty() && !chain.contains(model) {
                chain.push(model.clone());
            }
        }
        let state = self.state();
        let available: Vec<String> = chain
            .iter()
            .filter(|m| state.unavailable_models.get(*m).is_none_or(|at| at.elapsed() >= UNAVAILABLE_TTL))
            .cloned()
            .collect();
        if available.is_empty() {
            chain
        } else {
            available
        }
    }

    /// Executes a logical request with retries, fallback and metering.
    pub async fn execute(
        &self,
        request: GenerationRequest,
        cancel: &CancellationToken,
    ) -> Result<GenerationResponse, ProviderError> {
        let automatic = !request.retry.retry_rate_limits;
        if automatic {
            if let Some(until) = self.rate_limited_until() {
                return Err(ProviderError::RateLimited { retry_after: Some(until - Instant::now()), snapshot: None });
            }
            if self.is_offline() {
                return Err(ProviderError::Network("offline".into()));
            }
            if self.is_unauthorized() {
                return Err(ProviderError::Unauthorized);
            }
        }

        let started = Instant::now();
        let chain = self.model_chain(&request);
        let mut attempts = 0u32;
        let mut rate_limit_hits = 0u32;
        let mut last_model = chain.first().cloned().unwrap_or_default();
        let mut result: Result<GenerationResponse, ProviderError> = Err(ProviderError::Cancelled);

        'models: for model in &chain {
            last_model = model.clone();
            let mut attempt_for_model = 0u32;
            loop {
                if cancel.is_cancelled() {
                    result = Err(ProviderError::Cancelled);
                    break 'models;
                }
                attempts += 1;
                attempt_for_model += 1;
                let mut attempt = request.clone();
                attempt.model = model.clone();
                let outcome = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Err(ProviderError::Cancelled),
                    r = tokio::time::timeout(request.timeout, self.inner.generate(&attempt, cancel)) => {
                        r.unwrap_or(Err(ProviderError::Timeout))
                    }
                };
                match outcome {
                    Ok(response) => {
                        result = Ok(response);
                        break 'models;
                    }
                    Err(error @ ProviderError::ModelUnavailable { .. }) => {
                        self.state().unavailable_models.insert(model.clone(), Instant::now());
                        result = Err(error);
                        continue 'models;
                    }
                    Err(ProviderError::RateLimited { retry_after, snapshot }) => {
                        rate_limit_hits += 1;
                        let wait = retry_after.unwrap_or(DEFAULT_RATE_LIMIT_WAIT);
                        self.state().rate_limited_until = Some(Instant::now() + wait);
                        let policy = request.retry;
                        if policy.retry_rate_limits
                            && attempt_for_model < policy.max_attempts
                            && wait <= policy.max_rate_limit_wait
                        {
                            if sleep_or_cancel(wait, cancel).await {
                                continue;
                            }
                            result = Err(ProviderError::Cancelled);
                        } else {
                            result = Err(ProviderError::RateLimited { retry_after, snapshot });
                        }
                        break 'models;
                    }
                    Err(error) if error.is_transient() && attempt_for_model < request.retry.max_attempts => {
                        if sleep_or_cancel(request.retry.delay_for(attempt_for_model), cancel).await {
                            continue;
                        }
                        result = Err(ProviderError::Cancelled);
                        break 'models;
                    }
                    Err(error) => {
                        result = Err(error);
                        break 'models;
                    }
                }
            }
        }

        self.after_request(&request, &result, &last_model, started.elapsed(), attempts, rate_limit_hits);
        result
    }

    fn after_request(
        &self,
        request: &GenerationRequest,
        result: &Result<GenerationResponse, ProviderError>,
        last_model: &str,
        elapsed: Duration,
        attempts: u32,
        rate_limit_hits: u32,
    ) {
        let latency_ms = elapsed.as_millis().min(u128::from(u32::MAX)) as u32;
        {
            let mut state = self.state();
            match result {
                Ok(_) => {
                    state.network_failures = 0;
                    state.last_network_failure = None;
                    state.unauthorized = false;
                }
                Err(ProviderError::Unauthorized) => state.unauthorized = true,
                Err(ProviderError::Network(_) | ProviderError::Timeout) => {
                    state.network_failures += 1;
                    state.last_network_failure = Some(Instant::now());
                }
                Err(_) => {}
            }
            state.last_outcome = Some(RequestOutcome {
                feature: request.feature,
                model: result.as_ref().map_or_else(|_| last_model.to_string(), |r| r.model.clone()),
                status: result.as_ref().map_or_else(ProviderError::usage_status, |_| UsageStatus::Success),
                error_kind: result.as_ref().err().map(|e| e.kind().to_string()),
                latency_ms,
                at: Utc::now(),
                message: result
                    .as_ref()
                    .err()
                    .filter(|e| **e != ProviderError::Cancelled)
                    .map(ProviderError::user_message),
            });
        }
        if attempts == 0 || result.as_ref().err().is_some_and(|e| !e.reached_provider()) {
            return;
        }
        let event = match result {
            Ok(response) => UsageEvent {
                timestamp: Utc::now(),
                provider: self.inner.descriptor().id.to_string(),
                model: response.model.clone(),
                feature: request.feature,
                request_type: request.request_type,
                input_tokens: response.usage.map(|u| u.input_tokens),
                output_tokens: response.usage.map(|u| u.output_tokens),
                total_tokens: response.usage.map(|u| u.total_tokens),
                reasoning_tokens: response.usage.and_then(|u| u.reasoning_tokens),
                latency_ms: Some(latency_ms),
                provider_latency_ms: response.provider_latency.map(|d| d.as_millis().min(u128::from(u32::MAX)) as u32),
                status: UsageStatus::Success,
                error_kind: None,
                attempts,
                rate_limit_hits,
            },
            Err(error) => UsageEvent {
                timestamp: Utc::now(),
                provider: self.inner.descriptor().id.to_string(),
                model: last_model.to_string(),
                feature: request.feature,
                request_type: request.request_type,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                reasoning_tokens: None,
                latency_ms: Some(latency_ms),
                provider_latency_ms: None,
                status: error.usage_status(),
                error_kind: Some(error.kind().to_string()),
                attempts,
                rate_limit_hits,
            },
        };
        self.usage.record(event);
    }
}

/// Sleeps for `duration`; returns `false` if cancelled first.
async fn sleep_or_cancel(duration: Duration, cancel: &CancellationToken) -> bool {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => false,
        _ = tokio::time::sleep(duration) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::testing::{Script, ScriptedProvider};
    use crate::providers::types::*;
    use crate::usage::MemoryUsageSink;

    fn request(retry: RetryPolicy) -> GenerationRequest {
        GenerationRequest {
            model: "primary".into(),
            fallback_models: vec!["fallback".into()],
            messages: vec![ChatMessage::user("hi")],
            max_output_tokens: 16,
            temperature: 0.2,
            stop: vec![],
            response_format: ResponseFormat::Text,
            reasoning: ReasoningEffort::None,
            timeout: Duration::from_secs(5),
            feature: Feature::InlineCompletion,
            request_type: RequestType::Completion,
            retry,
        }
    }

    fn setup(scripts: Vec<Script>) -> (Arc<ScriptedProvider>, Arc<MemoryUsageSink>, ResilientProvider) {
        let provider = Arc::new(ScriptedProvider::new(scripts));
        let sink = Arc::new(MemoryUsageSink::default());
        let resilient = ResilientProvider::new(provider.clone(), sink.clone());
        (provider, sink, resilient)
    }

    #[tokio::test(start_paused = true)]
    async fn success_records_one_event_with_tokens() {
        let (_, sink, p) = setup(vec![Script::ok("continuation", 40, 8)]);
        let r = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await.unwrap();
        assert_eq!(r.text, "continuation");
        let events = sink.events();
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!((e.input_tokens, e.output_tokens, e.total_tokens), (Some(40), Some(8), Some(48)));
        assert_eq!((e.status, e.attempts, e.rate_limit_hits), (UsageStatus::Success, 1, 0));
        assert_eq!(e.feature, Feature::InlineCompletion);
        assert_eq!(e.model, "primary");
    }

    #[tokio::test(start_paused = true)]
    async fn retries_are_not_double_counted() {
        let (provider, sink, p) = setup(vec![
            Script::fail(ProviderError::Server { status: 503, message: "busy".into() }),
            Script::fail(ProviderError::Network("reset".into())),
            Script::ok("done", 100, 20),
        ]);
        let r = p.execute(request(RetryPolicy::interactive()), &CancellationToken::new()).await;
        assert!(r.is_ok());
        assert_eq!(provider.calls().len(), 3);
        let events = sink.events();
        assert_eq!(events.len(), 1, "one logical request, one event");
        assert_eq!(events[0].attempts, 3);
        assert_eq!(events[0].total_tokens, Some(120), "tokens from the successful attempt only");
    }

    #[tokio::test(start_paused = true)]
    async fn latency_sensitive_requests_do_not_retry() {
        let (provider, sink, p) = setup(vec![Script::fail(ProviderError::Server { status: 500, message: "x".into() })]);
        let r = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await;
        assert!(matches!(r, Err(ProviderError::Server { status: 500, .. })));
        assert_eq!(provider.calls().len(), 1);
        let e = &sink.events()[0];
        assert_eq!((e.status, e.error_kind.as_deref(), e.total_tokens), (UsageStatus::Error, Some("server"), None));
    }

    #[tokio::test(start_paused = true)]
    async fn unavailable_model_falls_back_and_is_remembered() {
        let (provider, sink, p) = setup(vec![
            Script::fail(ProviderError::ModelUnavailable { model: "primary".into() }),
            Script::ok("from fallback", 10, 2),
            Script::ok("second", 10, 2),
        ]);
        let r = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await.unwrap();
        assert_eq!(r.model, "fallback");
        assert_eq!(p.unavailable_models(), vec!["primary".to_string()]);
        // Next request goes straight to the fallback.
        p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await.unwrap();
        let models: Vec<String> = provider.calls().iter().map(|c| c.model.clone()).collect();
        assert_eq!(models, vec!["primary", "fallback", "fallback"]);
        let events = sink.events();
        assert_eq!(events.len(), 2);
        assert_eq!((events[0].model.as_str(), events[0].attempts), ("fallback", 2));
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limits_are_counted_and_respected() {
        let (provider, sink, p) = setup(vec![Script::fail(ProviderError::RateLimited {
            retry_after: Some(Duration::from_secs(20)),
            snapshot: None,
        })]);
        let r = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await;
        assert!(matches!(r, Err(ProviderError::RateLimited { .. })));
        let e = &sink.events()[0];
        assert_eq!((e.status, e.rate_limit_hits), (UsageStatus::RateLimited, 1));
        // Automatic requests now back off locally without calling the provider.
        let again = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await;
        assert!(matches!(again, Err(ProviderError::RateLimited { .. })));
        assert_eq!(provider.calls().len(), 1);
        assert_eq!(sink.events().len(), 1, "local backoff records nothing");
        tokio::time::advance(Duration::from_secs(21)).await;
        assert!(p.rate_limited_until().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn interactive_requests_wait_out_short_rate_limits() {
        let (_, sink, p) = setup(vec![
            Script::fail(ProviderError::RateLimited { retry_after: Some(Duration::from_secs(2)), snapshot: None }),
            Script::ok("ok", 5, 5),
        ]);
        let r = p.execute(request(RetryPolicy::interactive()), &CancellationToken::new()).await;
        assert!(r.is_ok());
        let e = &sink.events()[0];
        assert_eq!((e.status, e.attempts, e.rate_limit_hits), (UsageStatus::Success, 2, 1));
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_is_recorded_without_tokens() {
        let (_, sink, p) = setup(vec![Script::ok("late", 10, 10).with_delay(Duration::from_secs(2))]);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            c2.cancel();
        });
        let r = p.execute(request(RetryPolicy::none()), &cancel).await;
        assert_eq!(r, Err(ProviderError::Cancelled));
        let e = &sink.events()[0];
        assert_eq!((e.status, e.total_tokens), (UsageStatus::Cancelled, None));
        assert_ne!(p.last_outcome().unwrap().status, UsageStatus::Success);
    }

    #[tokio::test(start_paused = true)]
    async fn timeouts_are_reported() {
        let (_, sink, p) = setup(vec![Script::ok("slow", 1, 1).with_delay(Duration::from_secs(60))]);
        let r = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await;
        assert_eq!(r, Err(ProviderError::Timeout));
        assert_eq!(sink.events()[0].status, UsageStatus::Timeout);
    }

    #[tokio::test(start_paused = true)]
    async fn repeated_network_failures_switch_to_offline_mode() {
        let (provider, sink, p) = setup(vec![
            Script::fail(ProviderError::Network("down".into())),
            Script::fail(ProviderError::Network("down".into())),
            Script::ok("back", 1, 1),
        ]);
        for _ in 0..2 {
            let _ = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await;
        }
        assert!(p.is_offline());
        let skipped = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await;
        assert!(matches!(skipped, Err(ProviderError::Network(_))));
        assert_eq!(provider.calls().len(), 2, "offline requests are not sent");
        // Interactive requests still try, and success clears offline mode.
        p.execute(request(RetryPolicy::interactive()), &CancellationToken::new()).await.unwrap();
        assert!(!p.is_offline());
        assert_eq!(sink.events().len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn rejected_keys_stop_automatic_requests() {
        let (provider, sink, p) = setup(vec![Script::fail(ProviderError::Unauthorized), Script::ok("ok", 1, 1)]);
        let r = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await;
        assert_eq!(r, Err(ProviderError::Unauthorized));
        assert!(p.is_unauthorized());
        // Typing on does not keep sending requests with a rejected key.
        let skipped = p.execute(request(RetryPolicy::background()), &CancellationToken::new()).await;
        assert_eq!(skipped, Err(ProviderError::Unauthorized));
        assert_eq!(provider.calls().len(), 1, "automatic requests are not sent");
        assert_eq!(sink.events().len(), 1, "only the request that reached the provider is recorded");
        // A user-initiated request still goes through, and its success clears the state.
        p.execute(request(RetryPolicy::interactive()), &CancellationToken::new()).await.unwrap();
        assert!(!p.is_unauthorized());
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_key_clears_the_rejected_state() {
        let (_, _, p) = setup(vec![Script::fail(ProviderError::Unauthorized)]);
        let _ = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await;
        assert!(p.is_unauthorized());
        p.reset();
        assert!(!p.is_unauthorized());
    }

    #[tokio::test(start_paused = true)]
    async fn errors_that_never_reach_the_provider_are_not_recorded() {
        let (_, sink, p) = setup(vec![Script::fail(ProviderError::NotConfigured)]);
        let r = p.execute(request(RetryPolicy::none()), &CancellationToken::new()).await;
        assert_eq!(r, Err(ProviderError::NotConfigured));
        assert!(sink.events().is_empty());
        assert_eq!(p.last_outcome().unwrap().error_kind.as_deref(), Some("not_configured"));
    }
}
