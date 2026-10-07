# Usage system

Mote shows you how much AI you are using (requests, tokens, latency and estimated cost per feature and model) without ever storing what was asked or answered. This document covers how requests are metered, stored, aggregated and priced.

```text
ResilientProvider ──(one UsageEvent per logical request)──► UsageSink
                                                              │
                         UsageWriter thread ◄─────────────────┘
                              │ INSERT INTO usage_events          (metadata only)
                              ▼
                         SQLite ──► usage_buckets (local day × hour × provider × model × feature × status)
                              │   ──► latency_samples
                              ▼
          dashboard::build(buckets, samples, pricing, limits) ──► UsageDashboard ──► Usage page
```

## Metering

The accounting rule lives in one place, `ResilientProvider::execute` (`crates/mote-core/src/providers/resilient.rs`):

> **Every logical request that reaches the provider produces exactly one `UsageEvent`, however many HTTP attempts or fallback models it took.**

- Tokens come from the final successful attempt. Failed attempts add to `attempts` and `rate_limit_hits`, not to token counts.
- A request that never leaves the machine (no key, cloud AI off, local rate-limit backoff, offline mode, rejected key) records nothing.
- A request cancelled after it was sent records an event with status `cancelled` and no tokens, because the provider may still have done work. One cancelled before its first attempt records nothing.
- Latency is measured end to end (`latency_ms`, including retries and network); the provider's own processing time is kept separately (`provider_latency_ms`, Groq's `total_time`).

A `UsageEvent` holds:

| Field | Example |
|---|---|
| `timestamp` | 2026-10-07T17:13:09Z |
| `provider`, `model` | `groq`, `qwen/qwen3.8-27b` (the model that actually served it) |
| `feature` | `inline_completion`, `writing_assistance`, `prompt_enhancement`, `intent_classification`, `context_analysis`, `translation`, `rewrite`, `command_interface` |
| `request_type` | `completion`, `classification`, `transform` |
| `input_tokens`, `output_tokens`, `total_tokens`, `reasoning_tokens` | from the provider's `usage` object; reasoning tokens are part of the output count |
| `latency_ms`, `provider_latency_ms` | 412, 180 |
| `status`, `error_kind` | `success` · `error` · `rate_limited` · `timeout` · `cancelled`, with a machine-readable kind for failures |
| `attempts`, `rate_limit_hits` | 2, 1 |

There is no field for prompt or output text, and none can be added without a schema migration that would show up in review.

The tests in `resilient.rs` pin the rule down: a success records one event with tokens; three attempts record one event with `attempts = 3`; fallback records one event under the model that answered; rate limits are counted; cancellation records no tokens; local backoff, offline mode, a rejected key and missing configuration record nothing.

## Storage

`UsageWriter` (`apps/desktop/src-tauri/src/writers.rs`) receives events on a channel and inserts them from its own thread, so a model call never waits for the database. When **Usage analytics** is off (Settings → Privacy), events are dropped before they reach the channel. Rows older than the usage retention (180 days by default, 7–3,650) are deleted by the hourly maintenance task, and **Clear usage history** deletes them all.

Aggregation happens in SQL, in local time, so "today" and hourly charts match your clock:

- `Storage::usage_buckets(since)` groups by local day, hour, provider, model, feature and status, summing requests, tokens, latencies and rate-limit hits.
- `Storage::latency_samples(since, limit)` returns recent per-request latencies (successful requests) for medians and percentiles.

## Dashboard

`usage::dashboard::build` (pure, unit-tested) turns buckets, samples, the pricing catalog and the latest provider rate-limit snapshot into a `UsageDashboard`:

| Section | Contents |
|---|---|
| Summary | today, this week (from Monday), this month and the last 30 days: requests, failures, tokens, estimated cost, average latency |
| Breakdown (today and 30 days) | per feature and per model: requests, tokens, cost, average latency, and the busiest feature |
| Performance | average, median and p95 latency overall; average and median for completion and classification; failed requests, rate-limit events, timeouts, cancellations, error rate |
| Series | daily points for 30 days and hourly points for today |
| Provider limits | requests per day and tokens per minute, limit and remaining, as last reported by Groq |
| Unpriced models | models with usage but no price, so their cost is shown as unknown rather than zero |

The Usage page (`apps/desktop/src/app/sections/Usage.tsx`) presents it as summary tiles, the Groq rate-limit card, a note on what the numbers are, then a single range filter (Today / Last 30 days) that scopes everything below it: tokens, requests, estimated cost and latency charts, requests by feature, performance, and per-model usage. Every chart has a table view with the same numbers. Suggestion outcomes (shown, accepted, dismissed) come from activity metadata and appear only while activity retention is on.

**Mote's numbers vs. Groq's.** The dashboard is Mote's own record, built from the usage Groq returns with each response. Requests made with the same key from other tools don't appear, and Groq's billing can differ (free tier, discounts). The rate-limit card is the one place where Groq's view is shown, labelled as such.

## Pricing and cost estimates

Cost is an estimate: `input_tokens × input price + output_tokens × output price`, using the price in effect **on the day of the request**.

Prices are data, not code. The `model_pricing` table holds rows of provider, model, input and output price (USD per million tokens), effective date and source:

- **Built-in** rows ship with Mote (`usage/pricing.rs`), checked against groq.com/pricing on 2026-10-07:

  | Model | Input $/M | Output $/M |
  |---|---|---|
  | `openai/gpt-oss-20b` | 0.075 | 0.30 |
  | `openai/gpt-oss-120b` | 0.15 | 0.60 |
  | `qwen/qwen3.8-27b` | 0.80 | 4.00 |

- **User** rows are added in Settings → Pricing, to correct a price, add a model, or record a price change from a date onward. On the same date, a user row wins over a built-in one. Built-in rows can't be deleted, and **Reset pricing** removes only user rows.

`PricingCatalog::price_at(provider, model, date)` picks the latest row whose effective date is on or before the request's day. Usage older than the earliest row uses the earliest price. So a price change affects usage from its effective date on, and history keeps the prices that applied then.

Free tiers, cached-input discounts and batch pricing are not modelled, and the Usage page labels every cost as an estimate at list price.

## Privacy

Usage data is metadata about requests: counts, sizes, times, models and outcomes. It contains no prompts, outputs, typed text, clipboard contents, app names or window titles, and never leaves your computer. See the [privacy model](../privacy/privacy-model.md).
