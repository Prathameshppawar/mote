import type { ContextEvent } from "../bindings/ContextEvent";
import { FEATURE_LABELS, INTENT_LABELS } from "./format";

/** One-line, content-free description of an activity event. */
export function describeEvent(e: ContextEvent): string {
  switch (e.type) {
    case "application_changed":
      return `Switched to ${e.to}${e.from ? ` from ${e.from}` : ""}`;
    case "input_focused":
      return `Text field focused in ${e.app}`;
    case "intent_classified":
      return `Classified as ${INTENT_LABELS[e.kind] ?? e.kind} in ${e.app}`;
    case "clipboard_changed":
      return `Clipboard changed${e.source_app ? ` in ${e.source_app}` : ""} (${e.kind.replace("_", " ")}, ${e.char_count} characters; content not stored)`;
    case "suggestion_shown":
      return `Suggestion shown (${FEATURE_LABELS[e.feature] ?? e.feature})`;
    case "suggestion_accepted":
      return `Suggestion accepted (${FEATURE_LABELS[e.feature] ?? e.feature})`;
    case "suggestion_dismissed":
      return `Suggestion dismissed (${FEATURE_LABELS[e.feature] ?? e.feature})`;
    case "paused":
      return e.minutes ? `Paused for ${e.minutes} min` : "Paused";
    case "resumed":
      return "Resumed";
  }
}
