# 0002. Local-first: no Mote server, data stays on the device

- Status: Accepted
- Date: 2026-10-07

## Context

Mote sees what people type into every application: chats, email, documents, AI prompts. Whoever operates a backend in that path holds the most sensitive data a user has. At the same time, the useful features (completion, rewriting, prompt enhancement) need a capable language model, and good models are still faster and better in the cloud than on a typical laptop.

## Decision

- **No Mote backend.** There are no accounts, sync, telemetry or crash-reporting service. The app talks to exactly one remote service: the AI provider the user configures, with the user's own API key, directly from their computer.
- **All state is local.** Settings, usage metadata, pricing, exclusions and short-lived activity metadata live in one SQLite database in the user's application-data directory, readable only by that user. The API key lives in the OS credential store.
- **Metadata, not content.** Nothing typed, copied, prompted or generated is written to disk; content stays in memory for bounded times (see [0006](0006-privacy-boundaries.md)).
- **Local-first is not local-only inference.** AI runs at the configured provider (Groq in 1.0) for speed and quality with zero setup. A local model is optional future work through the provider abstraction ([0003](0003-provider-abstraction.md)), never a requirement.
- **Deterministic work stays local**: spelling, language detection, intent classification in most cases, context detection, usage aggregation and cost estimation cost no tokens and send nothing.

## Alternatives considered

- **A Mote cloud service proxying AI calls.** It would allow shared keys, billing and server-side improvements, but every keystroke-adjacent request would pass through infrastructure we operate, and it would require accounts. Rejected for privacy and for v1 scope (accounts, billing and sync are explicit v1 exclusions).
- **Mandatory local models (for example Ollama).** The strongest privacy, but it would mean gigabytes to download, a separate runtime to install, and latency and quality that vary with hardware. That's not acceptable as the default experience.

## Consequences

- Users bring their own Groq key, so onboarding includes a key step, and the dashboard shows usage against their own account's limits.
- No telemetry means we learn about problems only from reports. Diagnostics produce a sanitized report users can paste into an issue.
- No sync: settings and history stay on each machine.
- Uninstalling and "Reset local data" fully remove Mote's data; there is nothing held elsewhere.
