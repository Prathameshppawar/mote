import { useState } from "react";

import type { KeyboardSettings } from "../../bindings/KeyboardSettings";
import { Card, Kbd, Page, Row } from "../../components/controls";
import { acceleratorFromEvent, shortcutLabel } from "../../lib/shortcuts";
import { useSettings } from "../settingsContext";

/** Click, then press the new combination. Esc cancels. */
function ShortcutRecorder({
  value,
  onChange,
  label,
  error,
}: {
  value: string;
  onChange: (accelerator: string) => void;
  label: string;
  error?: string;
}) {
  const [recording, setRecording] = useState(false);
  return (
    <button
      type="button"
      className="btn"
      aria-label={
        recording
          ? `${label}: press the new shortcut, including Ctrl, Alt or Cmd. Escape cancels.`
          : `${label}: ${shortcutLabel(value)}. Click to change.`
      }
      aria-invalid={error ? true : undefined}
      aria-pressed={recording}
      onClick={() => setRecording(true)}
      onBlur={() => setRecording(false)}
      onKeyDown={(e) => {
        if (!recording) return;
        e.preventDefault();
        if (e.key === "Escape") return setRecording(false);
        const accelerator = acceleratorFromEvent(e.nativeEvent);
        if (accelerator) {
          setRecording(false);
          onChange(accelerator);
        }
      }}
      style={{ minWidth: 150 }}
    >
      {recording ? "Press keys…" : <Kbd>{shortcutLabel(value)}</Kbd>}
    </button>
  );
}

export function Keyboard() {
  const { settings, update, fieldError } = useSettings();
  if (!settings) return null;
  const k = settings.keyboard;
  const set = (key: keyof KeyboardSettings) => (accelerator: string) => update((s) => void (s.keyboard[key] = accelerator));
  return (
    <Page title="Keyboard" subtitle="Mote is keyboard-first. Suggestion keys are only taken while a suggestion is on screen.">
      <Card title="Global">
        <Row title="Command palette" help="Improve, rewrite, translate or summarize text in any app." error={fieldError("keyboard.commandPalette")}>
          <ShortcutRecorder label="Command palette" value={k.commandPalette} onChange={set("commandPalette")} error={fieldError("keyboard.commandPalette")} />
        </Row>
      </Card>
      <div className="section-label">While a suggestion is visible</div>
      <Card>
        <Row title="Accept" help="Inserts the suggestion. With nothing to accept, Tab passes through to the app.">
          <Kbd>Tab</Kbd>
        </Row>
        <Row title="Dismiss" help="Hides it; Mote won't suggest the same thing again for that text.">
          <Kbd>Esc</Kbd>
        </Row>
        <Row title="Next suggestion" help="Asks for an alternative continuation." error={fieldError("keyboard.nextSuggestion")}>
          <ShortcutRecorder label="Next suggestion" value={k.nextSuggestion} onChange={set("nextSuggestion")} error={fieldError("keyboard.nextSuggestion")} />
        </Row>
        <Row title="Previous suggestion" help="Goes back to the previous alternative." error={fieldError("keyboard.previousSuggestion")}>
          <ShortcutRecorder label="Previous suggestion" value={k.previousSuggestion} onChange={set("previousSuggestion")} error={fieldError("keyboard.previousSuggestion")} />
        </Row>
      </Card>
    </Page>
  );
}
