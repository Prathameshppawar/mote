# Testing

Mote's behaviour is decided in pure Rust behind traits, so almost all of it is tested without a desktop session, a network or an API key. What genuinely needs a real machine (reading other apps' text, typing into them) has a manual checklist.

## Test suites

| Suite | Where | What it covers |
|---|---|---|
| Core unit tests (180) | `crates/mote-core` | language detection, spelling, intent, privacy policy and redaction, settings validation, context events and insights, prompts and output cleaning, completion gating and suggestion sets, edit plans and clipboard restore, resilience and metering, pricing, dashboard aggregation, observer |
| Engine scenarios (16) | `crates/mote-core/src/engine/tests.rs` | the assistance loop end to end with virtual time, a fake platform and a scripted provider |
| Provider tests | `crates/mote-providers` | request bodies per model family (Qwen reasoning levels, GPT-OSS headroom, generic dialect), `<think>` stripping, chat-model filter, rate-limit header parsing, key redaction; 15 HTTP tests against a `wiremock` Groq (success, usage and limits, 401, decommissioned and unknown models, 429, 5xx retries metered once, timeouts, cancellation, unreachable server, keys never in errors) |
| Live Groq tests (6, ignored) | `crates/mote-providers/tests/live_groq.rs` | real requests: default models available, inline completion, invalid key rejected, JSON classification, Marathi completion stays in Latin script, unknown model reported unavailable |
| Storage tests (11) | `crates/mote-storage` | idempotent migrations and private file permissions, settings round trip and corruption recovery, every usage field, local-time buckets, dashboard end to end from stored events, pricing seeding and user overrides, exclusions, context retention, pruning, reset |
| Platform tests | `crates/mote-platform` | UTF-16 splitting (surrogate pairs, Devanagari), element keys, role mapping; on macOS keyboard chunking, clipboard sequence and running apps; on Windows the clipboard (including exclusion markers), process names and window enumeration |
| Desktop tests | `apps/desktop/src-tauri` | overlay placement, section routing, default shortcuts, home-directory hiding in diagnostics, secret store; binding export |
| Frontend tests (26) | `apps/desktop/src/**/*.test.ts(x)` | navigation and onboarding, saving settings, the usage dashboard (tiles, table twins, range scoping), formatters and accelerators, chart scales, activity descriptions without content, palette ordering, filtering and running, overlay rendering (Vitest, jsdom, Testing Library) |

Run them:

```sh
cargo test --workspace                       # all Rust suites
cargo test -p mote-core engine::             # just the engine scenarios
cd apps/desktop && npm test                  # frontend
```

## How the core is tested

- **Fakes, not mocks.** `FakePlatform` (`crates/mote-core/src/testing.rs`) holds a focused input, an app, a clipboard and a log of actions, and edits its text when Mote "types". `ScriptedProvider` returns scripted results or errors, optionally after a delay, and records calls. `FakeShell` records what the overlay was asked to show. `MemoryUsageSink` collects usage events.
- **Virtual time.** Engine and resilience tests use `#[tokio::test(start_paused = true)]`, so debounce, retry backoff, rate-limit waits and timeouts run instantly and deterministically.
- **Behaviour, not internals.** Engine tests type text into the fake platform and assert what appears in the overlay, what was typed into the app, which requests were made and which usage events were recorded.

### Usage accounting

The metering rule (one event per logical request) is the most important thing to keep correct. `crates/mote-core/src/providers/resilient.rs` tests:

- success records one event with tokens,
- three attempts record one event with `attempts = 3` and no double-counted tokens,
- latency-sensitive requests don't retry,
- fallback after an unavailable model records one event under the serving model, and the unavailable model is skipped next time,
- rate limits are counted, and local backoff records nothing,
- interactive requests wait out short rate limits,
- cancellation records an event without tokens,
- timeouts are reported,
- repeated network failures switch to offline mode, and a success clears it,
- a rejected key stops automatic requests until a new key or a successful interactive request,
- errors that never reach the provider record nothing.

`crates/mote-storage` checks that buckets aggregate by local day and hour, and `usage/dashboard.rs` checks totals, cost with dated prices, unpriced models, percentiles and period boundaries.

## Live tests against Groq

Live tests are ignored by default. To run them, provide a key through the environment or a file that is never committed:

```sh
export MOTE_GROQ_API_KEY=gsk_...     # or put the key in ~/.config/mote-dev/groq-api-key (chmod 600)
cargo test -p mote-providers --test live_groq -- --ignored --test-threads=1
```

They make a handful of small requests; with Groq's pricing that costs a fraction of a cent.

## CI

`.github/workflows/ci.yml` runs on every push to `main` and every pull request:

| Job | Runner | Steps |
|---|---|---|
| Frontend | Ubuntu | `npm ci`, typecheck, lint, test, build |
| Rust core | Ubuntu | `cargo fmt --check`, clippy and tests for the platform-independent crates |
| Desktop | macOS, Windows | frontend build, clippy and tests for the whole workspace; on macOS, binding freshness |
| Repository checks | Ubuntu | version consistency, credential scan, `npm audit` (production, high), `cargo audit` |

## Manual verification

Run this checklist on each platform before a release, using an installed build rather than `tauri dev`. Use test text only; nothing here needs real private data.

**Install and onboarding**
- [ ] The installer runs; the app launches into the menu bar or tray; onboarding opens.
- [ ] macOS: the Accessibility step opens System Settings, and the status updates after granting.
- [ ] A Groq key can be pasted and is verified, and an invalid key shows a clear message.

**Completion**
- [ ] In a native editor (TextEdit / Notepad), a browser textarea and a chat app, typing an unfinished sentence shows ghost text at the caret after a short pause.
- [ ] Tab inserts it; Esc hides it; ⌥] / Alt+] shows an alternative, ⌥[ / Alt+[ goes back.
- [ ] Typing the beginning of the suggestion shrinks it, and typing something else hides it.
- [ ] Tab and Esc behave normally in the app when no suggestion is visible.
- [ ] No completions in a terminal, code editor or search box.
- [ ] Hinglish input ("kal meeting ke baad hum") is continued in Hinglish.
- [ ] Typing works with a non-QWERTY layout (AZERTY or Dvorak) and with emoji or Devanagari in the suggestion.

**Writing**
- [ ] A misspelled word ("recieve ") is underlined in the pill and fixed with Tab; Esc stops suggesting that word.
- [ ] A risky sentence in an email draft gets a minimal grammar fix.

**Prompts and palette**
- [ ] In ChatGPT or Claude (app or browser) and in an IDE's AI chat (Claude Code, Copilot), pausing shows "Enhance prompt · Tab"; Tab replaces the prompt with an enhanced one, ⌘Z / Ctrl+Z restores it, and Esc cancels while it runs.
- [ ] The palette's enhancement styles return improved prompts.
- [ ] The palette opens with the shortcut, captures the selection, and Insert, Replace and Copy all work; the original app regains focus.
- [ ] After two multi-line insertions in a row, the clipboard still holds what you copied before.

**Context**
- [ ] Copy a stack trace in a browser, switch to an AI prompt in another app, and a "Use copied…" chip offers Debug error; picking it in the palette works.

**Privacy**
- [ ] Nothing is read or suggested in a password field, a password manager, or an app you excluded (by app and by window-title keyword).
- [ ] Pause for 1 hour stops all suggestions, and Resume restores them.
- [ ] With Cloud AI off, no requests are made (the Usage page stays unchanged) while spelling still works.
- [ ] Copy diagnostics produces a report without text, app names or keys.
- [ ] Reset local data removes settings, history and the API key.

**App coverage**
- [ ] Completions and the prompt enhancer work in VS Code (editor prose, chat inputs), Slack or another Electron app, and a text area on a web page in Chrome.

**Updates**
- [ ] Settings → About → Check for updates finds a newer published release, downloads it, and "Restart to Update" installs it; Accessibility keeps working after the restart.

**Usage and display**
- [ ] The Usage page updates after requests; the charts' table views match; costs are labelled as estimates.
- [ ] The overlay sits on the caret on a Retina or 150% display and on a second monitor, and never steals focus.
- [ ] Light and dark appearance both render correctly.
