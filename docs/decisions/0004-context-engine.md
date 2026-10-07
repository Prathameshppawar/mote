# 0004. Context engine: adaptive polling, structured events and a single-owner engine

- Status: Accepted
- Date: 2026-10-07

## Context

To help at the right moment, Mote needs to know which app is in front, which field has focus, the text around the caret, and what was recently copied. It must learn this quickly, across native, web and Electron apps, without slowing anything down and without seeing more than necessary. The logic that decides what to suggest has many timers and races (debounce, in-flight requests, typing during a request, shortcuts) and has to be correct.

## Decision

- **An adaptive polling observer** reads focus, text and the clipboard through the accessibility APIs (Accessibility on macOS, UI Automation on Windows): every 100 ms while you type, 250 ms with a field focused, 500 ms otherwise, and 1 s when blocked. It runs on its own thread, with a 0.25 s timeout on every macOS accessibility call.
- **Privacy is evaluated before every read** (enabled, paused, permission, Secure Input, password managers, Mote itself, exclusions, secure fields), and reads are bounded (2,000 characters before the caret, 200 after, 8,000 selected).
- **Clipboard changes are detected by sequence number**, and the text is read only on change, only if allowed, and never when marked concealed or monitor-excluded.
- **Structured events.** A `ContextManager` keeps a short-lived in-memory context window and emits typed `ContextEvent`s carrying only metadata (app switches, field focus, intent, clipboard kind and length, suggestion outcomes). These are persisted only within the user's retention setting.
- **The engine is a single-owner async actor.** One task receives observations, shortcuts, settings and model results through one inbox, so suggestion state has no locks. Model calls run in spawned, cancellable tasks, and results for text that has since changed are discarded.
- **Deterministic rules** for suggestions (completion gating, writing checks, prompt hints, context chips), tested with virtual time.

## Alternatives considered

- **Event subscriptions** (`AXObserver` notifications, UI Automation event handlers). These would be lower latency in theory, but support is inconsistent across apps (Chromium and Electron in particular), registrations leak or go stale when apps restart, and handler threading on Windows is error-prone. Polling with change detection is uniform and simple to reason about.
- **Global keyboard hooks.** They see every keystroke in every app, including passwords. That requires invasive permissions (Input Monitoring) and contradicts the privacy model. Rejected.
- **Screen capture with OCR.** Heavy, slow and maximally invasive, and screenshot-based vision is an explicit v1 exclusion.

## Consequences

- Mote notices typing within about 100 ms, well under the 450 ms completion debounce, and idles at negligible CPU.
- Apps that don't expose text to accessibility APIs can't be assisted; this is documented as a known limitation.
- The engine is fully testable with `FakePlatform`, `ScriptedProvider` and virtual time; 16 end-to-end scenarios cover its behaviour.
- Content (clipboard text, window titles) lives only in memory for bounded times. Pause, exclusion and "Clear context" drop it immediately.
