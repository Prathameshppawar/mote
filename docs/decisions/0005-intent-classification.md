# 0005. Intent and language: deterministic first, AI only when unsure

- Status: Accepted
- Date: 2026-10-07

## Context

The right help depends on what the user is writing. A chat reply wants a natural continuation and light grammar fixes. An AI prompt wants enhancement. Code, terminal commands, search boxes and forms want nothing. The app alone is not enough: a browser hosts all of these. Mote's users also write in Hinglish and romanized Hindi and Marathi, which English-centric tools "correct" into nonsense or translate unasked.

Classifying every field with a model would cost tokens and latency on every focus change, and would send text even when a local guess is obvious.

## Decision

**Intent** (`crates/mote-core/src/intent`):

- Kinds: conversation, prompt, code, command, note, search, form, unknown. Subtypes: email, chat, professional and casual messages; coding, research, reasoning and general prompts.
- `classify` combines weighted evidence:
  - the app category (bundle ID, executable or name; AI assistants recognised by window title in browsers),
  - the field's role, placeholder and label,
  - features of the text,
  - its language,
  - the recent clipboard kind,
  - the previous app.
- It returns a kind, subtype and confidence.
- Only when confidence is below **0.45**, the text has at least 24 characters, the kind isn't command or form, and AI classification is enabled, the classification model is asked once. It gets an excerpt of at most 400 characters plus field metadata, answers in JSON, and the result is cached for that field.

**Language** (`crates/mote-core/src/language`):

- Deterministic detection of English, Hinglish, romanized Hindi and Marathi, mixed English-Marathi, and Devanagari Hindi and Marathi, using script analysis, weighted lexicons and function words to find the matrix language of code-mixed sentences.
- Every model prompt carries an explicit instruction to keep the user's language and script. Only the Translate action may change language.
- Local spelling correction skips romanized Hindi and Marathi words and Indic-dominant text.

## Alternatives considered

- **Always ask a model.** It would be accurate, but it costs a request per field focus, adds latency before the first suggestion, and sends text even when the answer is obvious.
- **An on-device ML classifier.** It would add model files and inference machinery to the app for a problem that weighted signals solve well; results would also be harder to explain and test.
- **App-only rules.** These are too coarse: browsers, Electron apps and IDEs host every kind of field.
- **Off-the-shelf language identification.** Typical detectors label romanized Hindi as English or as a random language, and they don't model code-mixing.

## Consequences

- Most fields are classified locally in microseconds at no token cost; the AI fallback is rare and cached.
- Decisions are explainable (the signals are inspectable) and unit-tested, including Hinglish and Marathi cases.
- Unusual apps can be misclassified. The classifier already accepts a per-app user override, but exposing it in Settings is future work.
- Lexicons need curation as vocabulary grows; they live in code with tests.
