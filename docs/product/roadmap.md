# Roadmap

Plans, not promises. Nothing on this page is implemented yet; shipped work moves to the [changelog](../../CHANGELOG.md). The order reflects current priorities and will change with feedback.

## Next (1.x)

- **Trusted builds.** Developer ID signing and notarization on macOS, so Accessibility survives updates and Gatekeeper doesn't block the first launch. Code signing on Windows (for example Azure Trusted Signing).
- **Updates.** In-app update checks and signed auto-update.
- **Windows hardening.** Hands-on testing across common apps (Office, Teams, Slack, browsers), caret placement on mixed-DPI setups, and UWP edge cases.
- **Per-app preferences.** A Settings control for the intent override the classifier already supports ("treat this app as chat"), and per-app completion toggles.
- **More providers.** Generic OpenAI-compatible endpoints (the `Generic` dialect exists), and local models through OpenAI-compatible servers such as Ollama or LM Studio, always optional.
- **Remember dismissed words.** Words dismissed with Esc are ignored only until Mote restarts; save them to the permanent ignore list (Settings → Writing) instead.

## Later

- Hindi and Marathi spelling suggestions, in Latin and Devanagari script.
- Event-driven observation where an app supports it reliably, to reduce polling further.
- Richer context actions (for example "reply to this email" from a copied thread) with the same privacy rules.
- Exportable usage reports (CSV) for people who track AI spend.
- A Linux build, if the accessibility story there becomes workable.

## Not planned

The v1 exclusions still stand: no autonomous control or browsing, no screenshots, no accounts or cloud sync, and no telemetry.
