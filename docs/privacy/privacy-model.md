# Privacy model

Mote sits between you and every app you type in, so it is built to see as little as possible, keep less, and send only what a feature needs. This document describes exactly what Mote 1.0 observes, stores, sends and never touches. It reflects the code in `crates/mote-core/src/privacy.rs`, `observer.rs`, `crates/mote-storage` and `apps/desktop/src-tauri`.

## Principles

1. **Check before reading.** Every observation is evaluated by the privacy policy *before* any text or clipboard content is read. An excluded app is never read, even briefly.
2. **Read the minimum.** Only the focused text field, only around the caret, with hard size limits.
3. **Send only for a feature in use, to the provider you chose.** There is no Mote server and no telemetry.
4. **Store metadata, never content.** Nothing you type, copy, prompt or receive is written to disk.
5. **You stay in control.** Pause, exclusions, per-source switches, retention and full reset are one click away.

## What Mote observes

The observer polls the operating system (every 100 ms while you type, slowing to 1 s when idle or blocked) and evaluates the policy in this order:

| Check | Result if it fails |
|---|---|
| Assistance enabled and "Observe applications" on | Nothing is observed (`disabled`) |
| Not paused (tray → Pause for 1 hour, or until resumed) | Nothing is observed (`paused`) |
| Accessibility permission granted (macOS) | Nothing is observed |
| macOS Secure Input is off | Nothing is observed in any app |
| The app is not a password manager or Mote itself | The app is never read |
| No user exclusion matches the app or window title | The app is never read |
| The focused element is not a password/secure field | The field is never read |

Only when every check passes may Mote read:

- **The focused text field**: up to 2,000 characters before the caret and 200 after, plus up to 8,000 characters of selected text. Where the app supports range queries (most do), only those ranges are requested. For fields that don't, the operating system hands over the field's whole value, and Mote keeps only the bounded part around the caret and discards the rest immediately.
- **The field's role and placeholder** (for example "Message #general"), to tell a chat box from a search box.
- **The active application** (name, bundle identifier or executable) and **window title**. The title is used only to match exclusion rules and to recognise AI assistants in a browser tab. It is never stored or sent.
- **The clipboard**, if "Observe clipboard" is on: plain text only. Content that the copying app marks as concealed or transient (`org.nspasteboard.ConcealedType`, used by password managers) is never read. The clipboard is kept **in memory only** for 3 minutes by default (configurable from 10 seconds to 1 hour) to power "use copied content" suggestions.

Turning off **Observe text** stops reading text fields; turning off **Observe clipboard** stops reading the clipboard. Both are under Settings → Privacy.

### Always excluded

These exclusions are in code and cannot be turned off:

- Password managers: 1Password, Bitwarden, KeePassXC, LastPass, Dashlane, Keychain Access, Passwords and others.
- Password and other secure text fields in every application.
- Any application while macOS Secure Input is active (for example, while a terminal or login prompt has it enabled).
- Mote's own windows.

### Your exclusions

Settings → Excluded Apps lets you add:

- **Apps**, picked from running applications or entered by name, bundle identifier or executable (case-insensitive exact match).
- **Window-title keywords**, matched case-insensitively anywhere in the title. "bank" excludes every window whose title contains "bank", including a banking site in a browser tab.

## What is sent to the AI provider

Mote 1.0 uses Groq, over HTTPS, with your own API key. Requests go directly from your computer to `api.groq.com`; there is no intermediary. A request is made only when a feature needs one, and only if **Cloud AI** is enabled (Settings → Privacy) and a key is configured.

| Feature | When | What is sent |
|---|---|---|
| Inline completion | You pause typing (450 ms by default) in an eligible field | The last 600 characters before the caret, the app name, the detected context (for example "email") and a language instruction |
| Context classification | A field's purpose is unclear from local signals | An excerpt of at most 400 characters, the app name and category, the field role and placeholder |
| Grammar check | A finished sentence looks risky to the local heuristic | That one sentence and a language instruction |
| Palette actions and prompt enhancement | You choose an action | The selected text (or the field's text), the detected context and, for "use copied content" actions, the copied text and the name of the app it came from |

Never sent: window titles, other fields, other apps' content, your files, usage history, or anything from an excluded app.

Spelling correction, language detection, context detection and usage accounting run entirely on your computer and send nothing.

What Groq does with requests is governed by [Groq's privacy policy](https://groq.com/privacy-policy/) and your Groq account settings. Turn **Cloud AI** off and Mote makes no network requests at all apart from the ones you start explicitly from Settings (testing the connection, listing models).

## What is stored on your computer

Everything Mote persists lives in one SQLite database, created with permissions that only your user account can read (`0600` on macOS):

| Platform | Database | Logs (kept 7 days) |
|---|---|---|
| macOS | `~/Library/Application Support/io.github.prathameshppawar.mote/mote.db` | `~/Library/Logs/io.github.prathameshppawar.mote/` |
| Windows | `%APPDATA%\io.github.prathameshppawar.mote\mote.db` | `%LOCALAPPDATA%\io.github.prathameshppawar.mote\logs\` |

| Data | Contents | Kept |
|---|---|---|
| Settings | Your preferences (no API key) | Until changed or reset |
| Usage events | One row per AI request: time, provider, model, feature, request type, token counts, latency, status, error kind, attempts, rate-limit hits. **No prompt, no output.** | 180 days by default (7–3650); "Usage analytics" off records nothing |
| Activity metadata | App switches (app names and category), field focus (app name, field role), detected context, clipboard *kind* and length (never content), suggestion shown/accepted/dismissed, pause/resume. **No text, no window titles.** | 1 day by default; choose Off, 1 hour, 1 day or 1 week. Off stores nothing |
| Pricing | Built-in and your custom model prices | Until changed or reset |
| Exclusions | Your exclusion rules | Until removed or reset |

Expired rows are deleted hourly. You can review activity metadata under Settings → Context → Recent activity.

**The API key** is stored in the macOS Keychain or Windows Credential Manager (service `io.github.prathameshppawar.mote`, account `groq-api-key`), never in the database, settings or logs. In memory it is wiped when no longer needed.

**Logs** record events and errors with metadata only (counts, durations, model names, error kinds). They never contain typed text, prompts, outputs, clipboard contents, window titles or keys; anything that could carry a secret passes through a redaction filter first.

The app's web views keep the usual WebKit/WebView2 cache folders, but Mote's interface stores nothing in browser storage.

## What is never stored or logged

Typed text, selected text, prompts, model outputs, clipboard contents, window titles, field contents from any app, and API keys.

## Your controls

| Control | Where |
|---|---|
| Pause for an hour, or turn assistance off | Tray menu, Settings → General |
| Cloud AI on/off | Settings → Privacy |
| Observe applications / text / clipboard | Settings → Privacy |
| Exclusions | Settings → Excluded Apps |
| Activity retention, clear activity | Settings → Privacy / Context |
| Usage analytics, usage retention, clear usage history | Settings → Usage |
| Remove API key | Settings → AI Providers |
| **Reset local data**: deletes every database row, the API key and settings, then compacts the database | Settings → Privacy |

## Diagnostics

Settings → Diagnostics → **Copy diagnostics** produces a plain-text report for bug reports: version, OS, whether a key is configured (not the key), model names, provider health and rate limits, permission and observation state, database statistics, paths with your home folder shortened to `~`, average completion latency and the last request's status. It contains no text, prompts, clipboard contents, app names or keys, and is passed through the same redaction filter as logs.

## Uninstalling

Delete the app (macOS: drag Mote from Applications to the Trash; Windows: Settings → Apps → Mote → Uninstall). To remove its data as well, use **Reset local data** first, or delete:

- macOS: `~/Library/Application Support/io.github.prathameshppawar.mote`, `~/Library/Logs/io.github.prathameshppawar.mote`, `~/Library/Caches/io.github.prathameshppawar.mote`, `~/Library/WebKit/io.github.prathameshppawar.mote`, and the Keychain item `io.github.prathameshppawar.mote`.
- Windows: `%APPDATA%\io.github.prathameshppawar.mote`, `%LOCALAPPDATA%\io.github.prathameshppawar.mote`, and the Credential Manager entry `io.github.prathameshppawar.mote`.

On macOS, also remove Mote from System Settings → Privacy & Security → Accessibility.
