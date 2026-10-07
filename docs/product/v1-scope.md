# Version 1.0 scope

What Mote 1.0 includes, what it deliberately leaves out, and where its limits are. Everything listed as included is implemented and tested in this repository. Limits are listed honestly, and the [release notes](../releases/v1.0.0.md#known-limitations) repeat them for users.

## Included

| Area | In 1.0 | Where |
|---|---|---|
| Platforms | macOS 11+ (Apple Silicon and Intel), Windows 10/11 x64 | [platform layer](../architecture/platform-layer.md) |
| App shell | Menu bar / tray app, onboarding, settings, global command palette shortcut | `apps/desktop` |
| AI provider | Groq with your own key, stored in the OS credential store; provider, model and pricing configuration; connection test and model list | [provider system](../architecture/provider-system.md) |
| Context | Active application, focused field, clipboard (kind-aware, in memory), structured context events, contextual suggestions from copied content | [context engine](../architecture/context-engine.md) |
| Intent | Conversation vs prompt (and code, command, note, search, form) with subtypes; AI fallback when unsure | [ADR 0005](../decisions/0005-intent-classification.md) |
| Languages | English, Hinglish, romanized Hindi, romanized Marathi, mixed English-Marathi, Devanagari; preserved in every request | [context engine](../architecture/context-engine.md#language) |
| Writing | Local English spelling, AI grammar checks for risky sentences, palette rewrites and tone changes | |
| Completion | Inline ghost text, Tab acceptance, next and previous alternatives, dismissal memory, caching | |
| Prompts | Enhancement hint in AI assistants; Improve, Make precise, Make technical, Structure, Debug, Research, Explain, Expand context, Custom; Create prompt | |
| Privacy | Pause, cloud AI switch, per-source observation switches, app and window-title exclusions, always-excluded password managers and secure fields, retention, clear and reset | [privacy model](../privacy/privacy-model.md) |
| Storage | Local SQLite with migrations | `crates/mote-storage` |
| Usage | Per-request metering (exactly once), tokens including reasoning, latency, estimated cost with dated pricing, dashboard with breakdowns and provider limits | [usage system](../architecture/usage-system.md) |
| Diagnostics | Component health and a sanitized report | Settings → Diagnostics |
| Engineering | Tests across core, providers, storage, platform, desktop and UI; CI on macOS, Windows and Linux; automated installer builds; tag-driven GitHub Releases with checksums; documentation and decision records | [testing](../development/testing.md), [releasing](../development/releasing.md) |

## Excluded from 1.0

By design, not oversight:

- Autonomous computer control, autonomous browsing or remote agent execution
- Screenshots or computer vision
- Accounts, cloud sync, billing or team features
- Mobile apps or a browser extension
- A mandatory local model (Ollama or similar)

## Known limits of 1.0

- Builds are not code-signed or notarized; users confirm the first launch, and macOS asks for Accessibility again after each update.
- Windows has had less hands-on use than macOS; it is built, linted and unit-tested in CI.
- Only apps that expose text through accessibility APIs can be assisted.
- One provider (Groq). The default completion model is a Groq preview model, with an automatic fallback.
- English-only spelling suggestions; Hindi and Marathi words are preserved, not corrected.
- No per-app intent override in Settings yet (the classifier supports one).
- No auto-update and no Linux app.

What comes next is in the [roadmap](roadmap.md).
