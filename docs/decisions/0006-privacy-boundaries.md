# 0006. Privacy boundaries enforced in code

- Status: Accepted
- Date: 2026-10-07

## Context

A privacy policy that lives only in documentation erodes with every feature. Mote needs boundaries that are enforced at specific points in code, are easy to review, and fail closed.

## Decision

Mote defines what may be **observed**, **sent**, **stored** and **logged**, and enforces each at one place:

| Boundary | Enforcement point |
|---|---|
| Observe nothing unless allowed | `PrivacyPolicy::evaluate` runs in the observer *before* any text or clipboard read: assistance enabled, not paused, permission granted, no Secure Input, not a password manager, not Mote, no matching user exclusion (app or window-title keyword) |
| Never read secrets | Platform adapters report secure/password fields as secure without reading them, and never read clipboard content marked concealed or transient (macOS) or excluded from monitoring or history (Windows). The observer reads a clipboard change only if the app in front before and after it may both be observed, and the palette and Tab acceptance re-check the app in front before reading |
| Read the minimum | `ReadLimits` bound every read: 2,000 characters before the caret, 200 after and 8,000 selected for automatic assistance; 20,000 each for the command palette, read once when you open it and forgotten when it closes |
| Send only for a feature in use | `AiClient` refuses with `CloudDisabled` when the cloud switch is off. Each feature sends only its own input: completion the last 600 characters, a grammar check one sentence, classification an excerpt of at most 400 characters, palette actions the chosen text. Window titles are never sent |
| Store metadata only | The SQLite schema has no columns for typed text, prompts, outputs, clipboard contents or window titles. Activity metadata is pruned by retention (default 1 day; Off stores nothing); usage metadata by usage retention (default 180 days) |
| Keep secrets out of logs | API keys live in the OS credential store and in an `ApiKey` type that never prints and zeroes its memory; provider error messages and diagnostics pass through `redact_secrets`; logging guidelines forbid content at every level |
| Limit what each window can do | Tauri capabilities give the overlay and palette only the commands they need; strict CSP |

**Always excluded**, in code: password managers, secure fields everywhere, everything while macOS Secure Input is active, and Mote's own windows.

## Alternatives considered

- **User-managed exclusions only.** These are easy to forget. Password managers and secure fields must be excluded without configuration.
- **Keeping text history for smarter suggestions** (personal vocabulary, style). It would improve suggestions, but a store of everything a user types is a liability we chose not to create.
- **Sending window titles or broader context** to improve classification. The titles of email, document and browser windows often contain names and subjects. Titles are used locally only.

## Consequences

- The [privacy model](../privacy/privacy-model.md) can state guarantees precisely, and reviewers can check them at the enforcement points above.
- Some features are less clever than they could be: no long-term personalization, and classification without titles.
- Any change that reads, sends, stores or logs new data must update the privacy model and this record, and the pull request template asks about it.
