# 0003. Provider abstraction with Groq first, model roles and a resilience layer

- Status: Accepted
- Date: 2026-10-07

## Context

Inline completion only feels "in the flow" if a suggestion appears within a few hundred milliseconds of a pause in typing. Other features (prompt enhancement, rewriting) can take a second or two but benefit from stronger models. Providers fail in ordinary ways (rate limits, retired models, network drops), and models change faster than releases. Usage must be metered accurately whatever happens during a request.

## Decision

- **Groq is the 1.0 provider.** Its inference latency suits inline completion, it offers a free tier, and its API is OpenAI-compatible.
- **One small trait, `ModelProvider`**, with `generate`, `list_models`, `health_check` and `usage_snapshot`. Every feature is built on `generate`, so any provider that implements it supports all features. The HTTP implementation is `OpenAiCompatibleProvider` with a `Dialect` (Groq, Generic) for request quirks such as reasoning controls.
- **Model roles instead of hard-coded models.** Completion, classification, writing, reasoning and fallback each map to a configurable model. 1.0 defaults: `qwen/qwen3.8-27b` with reasoning disabled for completion, classification and writing; `openai/gpt-oss-120b` at low reasoning effort for prompt work; `openai/gpt-oss-20b` as the fallback.
- **A decorator owns cross-cutting behaviour.** `ResilientProvider` wraps any provider and applies per-feature retry policies, model fallback, rate-limit backoff, offline detection, rejected-key handling, cancellation, and metering of exactly one usage event per logical request ([0007](0007-usage-analytics.md)). Features don't implement any of this themselves.
- **No mandatory local model.** Ollama and other local servers expose OpenAI-compatible APIs and can be added as a provider later without touching features.

## Alternatives considered

- **Vendor SDKs per provider.** These mean more dependencies and different error models, and they don't help with the shared policy (retries, metering).
- **Calling the provider directly from each feature.** Simple at first, but retry, fallback and metering logic would be duplicated and, inevitably, inconsistent. That would make double-counted retries likely.
- **One model for everything.** A fast model gives weak prompt enhancement, and a strong model makes slow, expensive completions.

## Consequences

- Adding a provider means implementing the trait (or adding a dialect), settings, a keychain account and pricing rows. The engine, resilience and dashboard are unchanged.
- The default Qwen model is a preview model on Groq and can be retired at short notice. The fallback role and the 10-minute "unavailable model" memory keep features working, and the health check reports missing models in Settings.
- Retry policies are explicit per feature: none for completion (a late suggestion is useless), one quick retry for background work, and three attempts that can wait out short rate limits for user actions.
