# Provider system

Mote talks to language models through one small trait, wraps every call in a resilience and metering layer, and routes each feature to a model *role* rather than a hard-coded model. Groq is the provider in 1.0; the design leaves room for any OpenAI-compatible endpoint and for local models.

```text
feature (complete / classify / check_grammar / transform)
   │  AiClient: role → model, prompt, token budget, temperature, retry policy
   ▼
ResilientProvider: cloud switch, backoff, fallback, retries, cancellation, metering
   │                                         └──► UsageSink (exactly one UsageEvent)
   ▼
ModelProvider: GroqProvider → OpenAiCompatibleProvider (HTTP)
```

## The `ModelProvider` trait

`crates/mote-core/src/providers/mod.rs`:

```rust
#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    /// One generation attempt; must stop promptly when `cancel` fires.
    async fn generate(&self, request: &GenerationRequest, cancel: &CancellationToken)
        -> Result<GenerationResponse, ProviderError>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    /// Default: list models and report configured models that are missing.
    async fn health_check(&self, required_models: &[String]) -> HealthReport { … }
    /// Latest rate-limit information, if the provider reports it.
    fn usage_snapshot(&self) -> Option<RateLimitSnapshot> { … }
}
```

A `GenerationRequest` carries everything a call needs: model, fallback models, chat messages, output-token budget, temperature, stop sequences, response format (text or JSON object), reasoning effort, timeout, retry policy, and the `Feature` and `RequestType` it is accounted under. A `GenerationResponse` returns the text, finish reason, served model, provider request ID, token usage (input, output, total, reasoning), provider-side latency and the latest rate-limit snapshot.

Because every feature is built on `generate`, any provider that implements it supports all of Mote's features.

## Groq and OpenAI-compatible providers

`crates/mote-providers` implements `OpenAiCompatibleProvider`, parameterised by a `Dialect` (`Groq` or `Generic`). `GroqProvider` is a thin wrapper that fixes the dialect and base URL (`https://api.groq.com/openai/v1`, overridable in settings for proxies; `http://` is accepted only for localhost) and verifies keys.

What the dialect handles:

| Concern | Behaviour |
|---|---|
| Output budget | Sends `max_completion_tokens` |
| Reasoning, Qwen 3 models | `reasoning_effort: "none"` when the role asks for no reasoning (every role but reasoning). Otherwise the level (`low` / `medium` / `high`, or `default` on models without levels) with `reasoning_format: "hidden"` |
| Reasoning, GPT-OSS models | `reasoning_effort: low / medium / high` (reasoning can't be disabled, so "none" becomes `low`), `include_reasoning: false`, and extra output headroom so hidden reasoning cannot exhaust the budget |
| Inline `<think>` blocks | Stripped from the text before it reaches the engine |
| JSON mode | `response_format: { "type": "json_object" }` for classification |
| Usage | `prompt_tokens`, `completion_tokens`, `total_tokens`, `completion_tokens_details.reasoning_tokens`, Groq's `total_time` and request ID (`x_groq.id`) |
| Rate limits | `x-ratelimit-limit/remaining-requests` (per day), `x-ratelimit-limit/remaining-tokens` (per minute) and reset durations such as `1m26.4s`, parsed into a `RateLimitSnapshot` shown on the dashboard |
| Model list | `GET /models`, with speech, guard and embedding models filtered out |

HTTP errors are mapped to `ProviderError`:

| Response | Error | What Mote does |
|---|---|---|
| 401 / `invalid_api_key` | `Unauthorized` | Stops automatic requests until the key changes, and shows "needs API key" |
| 404 `model_not_found`, 400 `model_decommissioned` | `ModelUnavailable` | Skips the model for 10 minutes and uses the fallback |
| 429 | `RateLimited { retry_after }` | Backs off (see below) |
| 5xx, connection errors, timeouts | `Server` / `Network` / `Timeout` | Retried when the policy allows |
| other 4xx | `BadRequest` | Reported, not retried |
| unparsable body | `InvalidResponse` | Reported |

Provider error messages are truncated to 300 characters and passed through secret redaction before they can reach a log or the UI.

### API keys

The key is wrapped in `ApiKey`: `Debug` and `Display` print `[redacted]`, and the memory is zeroed on drop. It is stored in the OS credential store (macOS Keychain, Windows Credential Manager) under the service `io.github.prathameshppawar.mote`, never in settings or the database. Saving a key validates its shape, stores it, and runs a health check (list models, confirm the configured models exist) so Settings can show the result immediately. A health check also runs 3 seconds after launch when a key is configured and cloud AI is on.

## Model roles and routing

`AiClient` (`crates/mote-core/src/ai.rs`) maps each operation to a role. Each role maps to a model in Settings → Models:

| Role | Default model | Used for |
|---|---|---|
| Completion | `qwen/qwen3.8-27b` | Inline completion |
| Classification | `qwen/qwen3.8-27b` | Ambiguous field classification |
| Writing | `qwen/qwen3.8-27b` | Grammar checks; palette writing, tone, summarize, translate and continue actions |
| Reasoning | `openai/gpt-oss-120b` | Prompt enhancement, custom instructions, create prompt, explain, "use copied content" actions |
| Fallback | `openai/gpt-oss-20b` | Any role whose model is unavailable |

| Operation | Token budget | Temperature | Reasoning | Retry policy |
|---|---|---|---|---|
| `complete` | `min(2 × max words + 8, 96)` | 0.3 (0.8 for alternatives) | none | none: a late completion is useless |
| `classify` | 60 | 0.0, JSON | none | background |
| `check_grammar` | about twice the sentence's tokens + 32 | 0.1 | none | background |
| `transform` | sized to the action and input | 0.1 (fix, translate) to 0.7 (continue writing) | low for the reasoning role | interactive |

Every prompt is fenced (`<<<` … `>>>`) so text you are writing cannot be mistaken for instructions, and carries the language-preservation instruction from the [context engine](context-engine.md#language). Model output is cleaned before use: preambles, quotes, refusals and repeated text are removed, and completions are trimmed to whole words within the word limit.

## Resilience: `ResilientProvider`

`crates/mote-core/src/providers/resilient.rs` wraps the provider and applies the same policy to every request.

**Before sending.** `AiClient` refuses immediately with `CloudDisabled` or `NotConfigured`, so nothing leaves the machine. Automatic requests (completion, classification, grammar) also fail fast, recording nothing, while Mote is backing off from a rate limit, considers itself offline, or the provider has rejected the API key. A new key, or a successful request you started yourself, clears the rejected-key state.

**Retry policies:**

| Policy | Attempts per model | Backoff | Waits out 429s |
|---|---|---|---|
| `none` | 1 | none | no |
| `background` | 2 | 250 ms, max 1 s | no |
| `interactive` | 3 | 400 ms, doubling, max 4 s | yes, if `retry-after` ≤ 8 s |

**Fallback.** The request walks a chain of models: the role's model, then the fallback. A model that reported itself unavailable is skipped for 10 minutes.

**Rate limits.** Every 429 sets a local back-off until `retry-after` (5 s if absent). Interactive requests may wait it out; automatic ones fail fast, and the engine reports "rate limited" in the tray instead of hammering the API.

**Offline.** Two consecutive network failures or timeouts mark Mote offline; automatic requests are skipped for 30 seconds after each failure, and the first success clears the state.

**Cancellation.** Every attempt races the request's `CancellationToken`; backoff sleeps are cancellable too.

**Timeout.** Each attempt is bounded by the request timeout (20 s by default, 1–120 s in settings).

**Diagnostics.** The latest outcome (feature, model, status, error kind, latency; never content) is kept for Settings → Diagnostics.

**Metering.** After the request finishes, exactly one `UsageEvent` is recorded, if the request reached the provider. See [usage system](usage-system.md).

## Adding a provider

1. Implement `ModelProvider`. For an OpenAI-compatible service, construct `OpenAiCompatibleProvider` with `Dialect::Generic` (or add a dialect for its quirks).
2. Add a `ProviderKind` variant and its settings (base URL, timeout, model assignments) in `crates/mote-core/src/settings.rs`, with validation.
3. Store its credential under a new keychain account in `apps/desktop/src-tauri/src/secrets.rs`.
4. Add built-in pricing rows if it charges per token (`usage/pricing.rs`), or let users enter prices.
5. Expose it in Settings → AI Providers and the tray's provider menu.

The engine, resilience layer, metering and dashboard need no changes.
