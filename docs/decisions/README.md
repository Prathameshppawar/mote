# Architecture decision records

Significant decisions, with their context and trade-offs. When a decision changes, update its record (or supersede it with a new one) in the same pull request as the code.

| # | Decision | Status |
|---|---|---|
| [0001](0001-tauri.md) | Tauri 2 with a Rust core and a React interface | Accepted |
| [0002](0002-local-first.md) | Local-first: no Mote server, data stays on the device | Accepted |
| [0003](0003-provider-abstraction.md) | Provider abstraction with Groq first, model roles and a resilience layer | Accepted |
| [0004](0004-context-engine.md) | Context engine: adaptive polling, structured events and a single-owner engine | Accepted |
| [0005](0005-intent-classification.md) | Intent and language: deterministic first, AI only when unsure | Accepted |
| [0006](0006-privacy-boundaries.md) | Privacy boundaries enforced in code | Accepted |
| [0007](0007-usage-analytics.md) | Usage analytics: local metering at the resilience layer, dated pricing, honest estimates | Accepted |
| [0008](0008-updates-and-signing.md) | In-app updates from GitHub Releases, and a stable signing identity | Accepted |

New records use the next number and the same sections: Context, Decision, Alternatives considered, Consequences.
