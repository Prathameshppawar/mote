# 0007. Usage analytics: local metering at the resilience layer, dated pricing, honest estimates

- Status: Accepted
- Date: 2026-10-07

## Context

Users pay for tokens with their own key and want to know what Mote costs them: which features use the most, which models, how fast they are. The numbers must be right, with retries not double-counted and cancellations and failures represented, and they must not turn into a log of what users wrote. Prices change over time and differ by model.

## Decision

- **Meter in one place.** `ResilientProvider` records exactly one `UsageEvent` per logical request after it finishes, however many attempts or fallbacks it took. Tokens come from the final successful attempt, and attempts and rate-limit hits are counted separately. Requests that never reach the provider record nothing; cancelled requests record no tokens.
- **Metadata only.** An event holds time, provider, model, feature, request type, input/output/total/reasoning tokens, end-to-end and provider latency, status, error kind, attempts and rate-limit hits. There is no place for content.
- **Off the hot path.** Events go through a channel to a writer thread; model calls never wait on SQLite.
- **Aggregate in SQL, in local time**, into day and hour buckets per provider, model, feature and status. A pure, unit-tested `dashboard::build` turns them into summaries, breakdowns, performance stats and series.
- **Pricing is data with effective dates.** Built-in Groq list prices are dated (2026-10-07), and users can add or correct prices that apply from their effective date. Cost is always labelled an estimate, and models without a price are reported as unpriced rather than free.
- **Mote's view and the provider's view stay separate.** The dashboard shows Mote's own record of its requests. The rate-limit card shows Groq's reported limits, labelled as Groq's.
- **User control.** Usage analytics can be switched off (events are dropped), retention defaults to 180 days, and history can be cleared.

## Alternatives considered

- **The provider's usage reporting.** It would include requests made with the same key by other tools, and it can't attribute usage to Mote's features.
- **Counting tokens locally with a tokenizer.** Approximate, model-specific, and it can't see reasoning tokens. The provider's `usage` object is exact.
- **One event per HTTP attempt.** It double-counts retries, inflates request counts and misattributes fallbacks.

## Consequences

- Usage is accurate for Mote's own requests; tests pin the rules (no double counting, cancellation, failures, local backoff).
- Estimates can differ from invoices: free tiers, discounts and cached-input pricing aren't modelled, and the UI says so.
- Users must update prices when providers change them; dated rows keep history correct when they do.
