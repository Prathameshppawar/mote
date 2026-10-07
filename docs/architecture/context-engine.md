# Context engine

The context engine turns "what's on screen" into "what would help right now". It has four parts, all in `crates/mote-core`:

```text
observer.rs          OS → Observation          (polling thread, privacy first)
context/             Observation → ContextEvent (in-memory context window, metadata events)
intent/, language/   field + text → IntentAssessment, LanguageProfile
engine.rs            everything → suggestions   (async actor driving the overlay)
```

Every rule here is deterministic and unit-tested. The model is consulted only to classify an ambiguous field (rarely) and to produce text.

## Observer

`Observer::tick` runs on a dedicated thread. Each tick:

1. Reads the **active application** and window title.
2. Evaluates `PrivacyPolicy::evaluate(app, title, now)`. If the result is not `Allowed` (disabled, paused, excluded, password manager, Mote itself), it emits `Unobservable(reason)` and reads nothing else.
3. Checks the platform permission and macOS **Secure Input**; either one blocks reading.
4. Reads the **focused input** with `ReadLimits` (2,000 characters before the caret, 200 after, 8,000 selected). Secure fields are reported as secure without reading their value. The input must belong to the active application.
5. If clipboard observation is on, compares the clipboard **sequence number**. Only when it changed, the change was not Mote's own write, and both the app that was in front at the previous tick and the current one may be observed, does it read the text (plain text only, never concealed or monitor-excluded content). The copy is credited to the earlier app, because people copy and then switch. A change that happened while Mote was blocked is never read later.

Polling adapts to activity so idle CPU stays near zero:

| State | Interval |
|---|---|
| Text changed in the last 2 s | 100 ms |
| A text field is focused | 250 ms |
| No text field | 500 ms |
| Blocked (no permission, Secure Input, excluded) | 1 s |

Observations are sent to the engine only when something changed.

## Context manager and events

`ContextManager` keeps a short-lived picture of what you are doing, **in memory only**: the active and previous application, the focused field, and a `ClipboardSnapshot` (text, source app, kind, time) that expires after the clipboard TTL (180 s by default). Pause, exclusion and "Clear context" drop it immediately.

From observations it emits `ContextEvent`s, structured metadata with no content:

| Event | Fields |
|---|---|
| `application_changed` | from, to, app category |
| `input_focused` | app, field role |
| `intent_classified` | app, kind, confidence |
| `clipboard_changed` | source app, clipboard kind, character count |
| `suggestion_shown` / `_accepted` / `_dismissed` | feature |
| `paused` / `resumed` | minutes |

Events are persisted by the context writer thread only when activity retention is not Off, and they power Settings → Privacy → Recent activity and the suggestion acceptance statistics on the dashboard. Each run of focus on one field is also summarised as a `context_sessions` row (app, role, intent, suggestions shown/accepted/dismissed).

### Clipboard kinds

`context/clipboard.rs` classifies copied text without storing it: `url`, `path`, `json`, `stack_trace`, `email`, `code`, `short_text`, `text`. The kind (not the content) is what gets recorded.

## Intent

*What kind of writing is this?* drives everything else: completion is offered in conversations, prompts and notes but never in code, commands, search boxes or forms; grammar checks run in conversations and notes; the prompt hint appears in prompts.

| Kind | Subtypes |
|---|---|
| `conversation` | `email`, `chat`, `professional_message`, `casual_message` |
| `prompt` | `coding`, `research`, `reasoning`, `general` |
| `code`, `command`, `note`, `search`, `form`, `unknown` | |

`intent::classify(IntentSignals)` adds weighted evidence from:

- **The application category** (`intent/apps.rs`): email, chat, AI assistant, IDE, terminal, notes, browser, launcher, password manager, Mote, other. Apps are recognised by bundle identifier, executable and name, and AI assistants in a browser by the window title ("ChatGPT", "Claude", "Gemini", "Perplexity", …).
- **The field**: role (text area, text field, search field, combo box, document, terminal), whether it is multi-line, its placeholder and label ("Message #general", "Ask anything", "Search"). In IDEs, an assistant's chat box is recognised by its accessible label (Claude Code's "Message input", VS Code's "Chat input"), because those boxes draw their placeholder without exposing it.
- **The text**: greetings and sign-offs, imperative instructions, code tokens, shell syntax, question shape.
- **Language profile**, **recent clipboard kind**, and **the previous app** (copy in a browser → paste into an IDE prompt).

(`IntentSignals` also accepts a per-app user override, which wins outright; 1.0 doesn't expose a setting for it yet.)

The result is an `IntentAssessment { kind, subtype, confidence, source }`. If confidence is below **0.45**, the text has at least 24 characters, the kind isn't `command` or `form`, and "AI classification" is enabled, the engine asks the classification model once. It sends an excerpt of at most 400 characters plus the field's metadata and caches the answer for that field, so a field is classified by AI at most once per focus.

## Language

`language::detect` returns a `LanguageProfile` with a label (`english`, `hinglish`, `hindi_latin`, `mixed_english_hindi`, `marathi_latin`, `marathi_english`, `mixed_english_marathi`, `hindi_devanagari`, `marathi_devanagari`, `other`, `unknown`), script shares and confidence.

Detection is deterministic: Devanagari is identified by script and distinguished as Marathi or Hindi by marker words; romanized text is scored against weighted Hindi and Marathi lexicons, and *function words* ("ka", "hai", "kar", "aahe", "la", …) decide the matrix language of code-mixed sentences such as "bhai meeting ka time change kar do please". The share of common English words keeps typo-heavy English from being misread as something else.

The profile is used to:

- add a **preservation instruction** to every model prompt. For Hinglish: "The text is Hinglish (Hindi mixed with English, Latin script). Keep the same Hinglish mix and Latin script. Do not translate it into English."
- keep **spelling correction** away from romanized Hindi and Marathi words and from Indic-dominant text,
- pick grammar-check candidates only where a check makes sense.

## Insights

`context/insights.rs` turns recent activity into an optional suggestion chip, deterministically and without calling the model. A chip is offered when:

- you copied **substantial** content (a stack trace, code, JSON, a URL, a path, an email or longer text) of at least 40 characters,
- **in a different app** from the one you are in now,
- **within the last 3 minutes**,
- and you have typed at most 60 characters into the current field.

The chip reads "Use copied … from Chrome" and lists actions that fit where you are:

| You are in | Copied | Actions |
|---|---|---|
| An AI prompt | a stack trace | Debug error · Analyze issue · Create prompt |
| An AI prompt | code or JSON | Explain code · Analyze issue · Create prompt |
| An AI prompt | anything else | Create coding task (in an IDE or AI assistant) · Analyze issue · Summarize · Create prompt |
| A conversation | anything | Draft response · Summarize |
| A note | anything | Summarize · Create prompt |
| An unclassified field in an IDE or AI assistant | anything | Create coding task · Summarize |

Nothing is sent anywhere unless you pick an action in the palette.

## Engine

`Engine` is an async actor (`engine.rs`) with one inbox (`EngineInput`): observations, shortcut presses, settings changes, "clear context", and results of the model tasks it spawned. Because it alone owns the context window and suggestion state, there are no locks around assistance logic.

### Scheduling

After each observation the engine schedules at most one of each:

| Timer | Fires after | Then |
|---|---|---|
| Completion | debounce (450 ms) since the last keystroke | `should_complete` → cache → request |
| Writing check | 700 ms after a word boundary, 1.6 s mid-word | local spelling; else grammar candidate → AI check |
| Prompt hint | 2 s, with at least 15 characters in an AI prompt | show "Enhance prompt · Tab" (replacing a completion still on screen) |
| Context chip | immediately when an insight applies | expires after 8 s |

`completion::should_complete` requires completion to be enabled for the intent, the caret to be at the end, no selection, no trailing newline, at least `min_chars` (12) and two words, an unfinished sentence, at least `min_interval_ms` (1.2 s) since the previous request, and no recent dismissal at this point. If only the interval blocks it, the engine reschedules for the moment the interval ends.

### Requests and stale results

Each request runs in a spawned task with a `CancellationToken`. Typing past the anchor cancels it; a focus change cancels everything. When a result arrives, the engine checks it still applies to the current text before showing it, and drops it otherwise. Results are cached per field and text (128 entries), so returning to the same point costs nothing.

### Suggestion lifecycle

- **Shown.** The overlay renders ghost text at the caret (completions) or a pill below it (corrections, hints, context). Tab, Esc and ⌥] / ⌥[ are registered as global shortcuts only now, and released when the suggestion goes away.
- **Typing through.** If you type the beginning of the suggestion, it shrinks to the rest (`SuggestionSet::on_text`). Typing all of it counts as accepted. Typing something else hides it.
- **Alternatives.** ⌥] asks for up to three alternatives (at a higher temperature, avoiding the ones already shown) and cycles through them.
- **Accept (Tab).** The field is re-read. If the text still ends with what the suggestion was made for, the edit is applied (see below); otherwise Tab is passed through to your app.
- **Enhance (Tab on the hint).** The whole prompt box is read (up to 12,000 characters on each side of the caret; a longer prompt is left to the palette), enhanced with the default style while "Enhancing prompt…" shows, and written back with `ReplaceAll` only if the field still holds exactly the text that was sent. Typing or Esc cancels; if the field can't be edited, the result goes to the clipboard. A notice confirms the outcome ("Prompt enhanced · ⌘Z to undo").
- **Dismiss (Esc).** The suggestion is hidden. The completion anchor is remembered so it isn't offered again, and a dismissed spelling correction isn't offered again for that word until Mote restarts. The permanent ignore list is under Settings → Writing.

### Applying edits

`assistance::apply_edit` turns an `EditPlan` into platform calls:

| Plan | How |
|---|---|
| `Insert` (single line) | typed as Unicode key events, layout-independent |
| `Insert` (multi-line) | pasted through the clipboard (typing a newline would send a chat message), or typed line by line with Shift+Enter if the app has no paste command or the clipboard holds images or files |
| `ReplaceSelection` | inserted over the current selection, as above |
| `ReplaceBeforeCaret` | re-reads the field, checks the expected tail is still there, deletes it with Backspace and inserts the correction |
| `ReplaceAll` | selects everything (on macOS by setting the accessibility selection, falling back to the app's Select All menu item), then inserts |

When pasting, the clipboard's previous text is saved, the paste buffer is written as *transient* (hidden from clipboard history), and the user's text is put back afterwards if nothing else changed the clipboard in the meantime. Mote records its own clipboard writes so the observer doesn't mistake them for a copy.

## Testing

`engine/tests.rs` drives the actor with virtual time (`tokio::time::pause`), a `FakePlatform` and a `ScriptedProvider`. Its 16 scenarios include: a completion suggested and accepted with Tab; typing through a suggestion until the text diverges; discarding a result that arrives after the text changed; not re-requesting dismissed suggestions; alternatives with next and previous; spelling corrections applied; AI grammar checks only for risky sentences; Hinglish completed in Hinglish; no completions in code or terminals; excluded apps ignored entirely; Tab passed through without a suggestion or when the suggestion is stale; a copied email offered as context in an IDE prompt; rate limits pausing automatic requests; the prompt hint shown once; and a missing API key reported without any request.

The observer, intent, language, spelling, insights and edit logic each have their own unit tests.
