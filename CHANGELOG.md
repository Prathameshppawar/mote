# Changelog

All notable changes to Mote are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Mote uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.2] - 2026-10-08

### Fixed

- Opening Mote from Applications, Spotlight or Launchpad while it was already running did nothing, which left no way in when the menu bar icon was hidden behind the notch. It now opens Settings (macOS).

## [1.1.1] - 2026-10-08

### Fixed

- Claude Code's prompt box and VS Code's chat inputs (Copilot Chat, inline chat) are recognized as prompts by their accessible labels, so completions and "Enhance prompt" appear there. Their placeholders aren't visible to accessibility tools, so 1.1.0 classified them as code unless AI classification decided otherwise.

## [1.1.0] - 2026-10-08

### Added

- **Prompt enhancer on Tab.** In AI prompt boxes, pause for a moment and "Enhance prompt · Tab" appears; Tab rewrites the whole prompt in place (⌘Z / Ctrl+Z restores yours). Esc cancels or hides the hint for that field, typing during enhancement keeps your version, and if a field can't be edited the enhanced prompt goes to the clipboard. Choose the style used by Tab under Settings → Context (default: Improve).
- **Updates in place.** Mote checks GitHub Releases a minute after launch and every six hours, downloads and verifies a newer version in the background, and installs it when you choose "Restart to Update" in the menu bar or Settings → About. Turn it off under Settings → General.
- Prompt boxes recognized in more places: AI chat inputs inside IDEs (Claude Code, GitHub Copilot) and the Microsoft 365 Copilot app.

### Changed

- macOS builds are signed with a stable Mote certificate instead of ad hoc, so macOS keeps the Accessibility permission across updates. Moving from 1.0 to 1.1 asks for it once more.

### Fixed

- Mote could miss the text field in Electron apps (VS Code, Slack, Notion, Discord), Chrome-based browsers and apps embedding Chromium or WebView2: it now switches on their accessibility tree before looking for the focused field, as these apps require.

## [1.0.0] - 2026-10-07

First public release.

### Added

- **Inline completion.** Ghost-text continuations at the caret in chat, email, prompt and note fields. Debounced (450 ms by default), cancelled the moment you keep typing, cached, and never blocking input. Tab accepts, Esc dismisses, ⌥] / ⌥[ (Alt on Windows) cycles up to three alternatives, and a dismissed suggestion is not offered again at the same spot.
- **Writing assistance.** Local spelling correction with an 82,765-word frequency dictionary and an edit-distance error model (no network, no tokens), and AI grammar checks for single sentences that a local heuristic flags as risky. Corrections are minimal and keep your wording.
- **Prompt enhancement.** In AI assistants (ChatGPT, Claude, Gemini, Perplexity and others, in their apps or a browser tab), a hint offers to enhance the prompt. Styles: Improve, Make precise, Make technical, Structure, Debug, Research, Explain, Expand context, and a custom instruction. "Create prompt" turns any text into a prompt.
- **Command palette** (⌘⇧Space / Ctrl+Shift+Space, configurable). It works on the selection or the focused field: fix spelling and grammar, improve, rewrite, professional, casual, clearer, concise, summarize, explain, translate, continue writing. Results can be inserted, replaced or copied.
- **Context intelligence.** Each text field is classified locally (conversation, prompt, code, command, note, search, form) with subtypes such as email, chat, coding or research prompt. An AI classifier is consulted only when local confidence is low. Copied errors, stack traces, code, JSON, paths and links are recognised and offered as next steps in another app ("Debug this error", "Create coding task", "Analyze issue", …).
- **Mixed-language support.** English, Hinglish, romanized Hindi and Marathi, mixed English-Marathi and Devanagari are detected per field. Every AI request is told to keep the user's language and script, and Mote never translates unless asked.
- **Groq provider** behind a provider abstraction, with model roles (completion, classification, writing, reasoning, fallback), reasoning-effort control for Qwen and GPT-OSS models, connection testing and model listing.
- **Resilience.** Per-feature retry policies, automatic fallback to a second model, rate-limit backoff that honours `retry-after`, offline detection, request cancellation, and an assistance engine that restarts itself after an internal error without leaving Tab or Esc captured.
- **Usage dashboard.** Requests, tokens (input, output, reasoning), median and p95 latency, error rate, rate-limit events and estimated cost. It covers today, 7 and 30 days, with breakdowns by feature and model, daily and hourly charts with table views, provider rate limits, and a privacy note. Exactly one usage record per logical request, however many retries it took.
- **Cost estimation** from a pricing table with effective dates. Built-in Groq list prices are dated 2026-10-07; you can add or correct prices, and they apply from their effective date. Costs are always labelled as estimates.
- **Privacy controls.** Pause (tray), cloud AI switch, per-source observation switches (applications, text, clipboard), app and window-title exclusions, activity retention (off, 1 hour, 1 day, 1 week), usage retention, clear and reset actions.
- **Settings.** General, AI Providers, Models, Pricing, Completion, Writing, Context, Privacy, Usage, Keyboard, Excluded Apps, Diagnostics and About, with validation of every field.
- **Onboarding.** What Mote does, what it can access, permission setup, API key and shortcuts.
- **Tray / menu bar.** Status, assistance, completion and context toggles, provider, command palette, usage, settings, privacy, diagnostics, pause for an hour, quit.
- **Diagnostics.** Component health, provider status and limits, permission state, database integrity, recent request outcome, and a sanitized report to copy into bug reports.
- **Platforms.** macOS 11+ (Apple Silicon and Intel) via the Accessibility API; Windows 10/11 x64 via UI Automation. Typing is layout-independent on both. On macOS, paste and select-all run through the app's own menu commands.
- **Releases.** CI on macOS, Windows and Linux, a tag-driven release pipeline that builds macOS DMGs and Windows NSIS/MSI installers, and SHA-256 checksums for every file.

### Security

- API keys are stored only in the macOS Keychain or Windows Credential Manager, are wiped from memory after use, and are redacted from logs, errors and diagnostics.
- Password managers, password fields, macOS Secure Input and Mote's own windows are never observed. The clipboard honours concealed/transient markers (macOS) and monitor-exclusion markers (Windows).
- Typed text, prompts, model outputs, clipboard contents and window titles are never stored or logged. The database is readable only by your user account.
- Each window gets only the IPC commands it needs (Tauri capabilities), under a strict content security policy.

[Unreleased]: https://github.com/Prathameshppawar/mote/compare/v1.1.2...HEAD
[1.1.2]: https://github.com/Prathameshppawar/mote/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/Prathameshppawar/mote/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/Prathameshppawar/mote/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/Prathameshppawar/mote/releases/tag/v1.0.0
