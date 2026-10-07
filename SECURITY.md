# Security policy

Mote reads text from the apps you type in and holds an API key, so security reports are taken seriously and handled privately.

## Supported versions

| Version | Supported |
|---|---|
| 1.0.x | ✓ |

Fixes are released as new patch versions; please update to the latest release.

## Reporting a vulnerability

**Please don't open a public issue for security problems.**

Report privately through GitHub: open the repository's **Security** tab and choose **Report a vulnerability**. If that option isn't available to you, contact the maintainer, [@Prathameshppawar](https://github.com/Prathameshppawar), privately at the email address on their GitHub profile.

Please include:

- what an attacker could do, and under which conditions,
- steps to reproduce, with the Mote version and OS,
- the diagnostics report (Settings → Diagnostics → Copy diagnostics) if relevant. It contains no text or keys.

You'll get an acknowledgement as soon as possible and updates as the fix progresses. Credit is given in the release notes unless you prefer otherwise.

## What counts

Especially welcome:

- Mote reading text or clipboard content it shouldn't: password fields, password managers, excluded apps, content while paused, or anything during macOS Secure Input.
- Typed text, prompts, model output, clipboard contents, window titles or API keys reaching disk, logs, diagnostics or anywhere other than the configured AI provider.
- API key exposure (memory, logs, IPC, error messages).
- Ways for the overlay or palette windows, or web content, to call IPC commands beyond their capabilities, or to escape the content security policy.
- Text inserted somewhere other than where the user accepted it.
- Problems with the integrity of release artifacts or the release pipeline.

Out of scope:

- Gatekeeper and SmartScreen warnings for the unsigned 1.0 builds (documented).
- What the AI provider does with requests, which is governed by its own policies.
- Attacks that require an already-compromised user account or administrator access.

## How Mote protects you

- **No content at rest.** Only metadata is stored (settings, usage counts, short-lived activity metadata) in a database readable only by your user account.
- **Secrets in the OS credential store.** The Groq key lives in the macOS Keychain or Windows Credential Manager, is wrapped in a type that never prints and is zeroed after use, and is redacted from logs, errors and diagnostics.
- **Privacy checks before reading.** Password managers, secure fields, Secure Input and excluded apps are never read (see the [privacy model](docs/privacy/privacy-model.md) and [ADR 0006](docs/decisions/0006-privacy-boundaries.md)).
- **Least privilege between windows.** Each window may call only the IPC commands it needs (Tauri capabilities), under a strict content security policy (only the app's own scripts; network access only to the IPC endpoint).
- **Two outbound destinations.** HTTPS to the configured provider (`api.groq.com` by default; plain HTTP is accepted only for loopback addresses such as `localhost`, and never with credentials in the URL), and GitHub Releases for update checks, which send no data about you and can be turned off.
- **Signed updates.** Updates install only if they verify against the updater public key built into the app; the private key lives only in the release workflow's secrets.
- **Dependency checks in CI.** `npm audit` (production dependencies, high severity) and `cargo audit` run on every push and pull request. As of 1.0, `cargo audit` reports no vulnerabilities and two informational warnings (RUSTSEC-2024-0429 `glib`, unsound; RUSTSEC-2024-0370 `proc-macro-error`, unmaintained). Both are in Tauri's Linux-only GTK dependencies and are not compiled into the macOS or Windows apps.
- **Verifiable releases.** Every release publishes `SHA256SUMS.txt`. Since 1.1, macOS builds are signed with Mote's own (self-signed) certificate, so their identity stays stable across updates; they are not notarized by Apple, and Windows installers are not code-signed yet (see the [roadmap](docs/product/roadmap.md)).

## A note for contributors

Never log content. Use `redact::describe(text)` for lengths, keep secrets in `ApiKey`, and run `bash scripts/check-secrets.sh` before pushing. The pull request template has a privacy checklist.
