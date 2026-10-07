import { Card, Note, Page, Row, Segmented, ToggleRow, Toast } from "../../components/controls";
import { useToast } from "../../components/useToast";
import { INTENT_LABELS, SUBTYPE_LABELS, formatClock } from "../../lib/format";
import { api, errorMessage } from "../../lib/ipc";
import type { Theme } from "../../bindings/Theme";
import { useSettings } from "../settingsContext";
import { STATE_TEXT, stateTone, useEngineStatus } from "../engineStatus";

const LANGUAGE_NAMES: Record<string, string> = {
  english: "English",
  hindi_latin: "Hindi (Latin script)",
  hinglish: "Hinglish",
  mixed_english_hindi: "English + Hindi",
  marathi_latin: "Marathi (Latin script)",
  marathi_english: "Marathi + English",
  mixed_english_marathi: "English + Marathi",
  hindi_devanagari: "Hindi",
  marathi_devanagari: "Marathi",
  other: "Other",
  unknown: "—",
};

export function General() {
  const { settings, update, replace, error } = useSettings();
  const status = useEngineStatus();
  const [toast, showToast] = useToast();
  if (!settings) return null;
  const pausedUntil = settings.general.pausedUntil && new Date(settings.general.pausedUntil) > new Date() ? settings.general.pausedUntil : null;

  const pause = async (minutes: number | null) => {
    try {
      replace(await api.setPaused(minutes));
      showToast(minutes ? `Paused for ${minutes >= 60 ? `${minutes / 60} h` : `${minutes} min`}` : "Resumed");
    } catch (e) {
      showToast(errorMessage(e));
    }
  };

  return (
    <Page title="General" subtitle="Mote runs quietly in your menu bar and helps where you write. These are the essentials.">
      <Card title="Status" subtitle="What Mote sees right now (in memory only).">
        <div className="row">
          <div className="row-text">
            <div className="row-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span className="status-dot" data-tone={status ? stateTone(status.state) : undefined} />
              {status ? STATE_TEXT[status.state] : "Loading…"}
            </div>
            <div className="row-help">
              {status?.message ??
                (status?.app
                  ? `${status.app}${status.intent ? ` · ${INTENT_LABELS[status.intent]}` : ""}${status.subtype ? ` (${SUBTYPE_LABELS[status.subtype]})` : ""}${status.language ? ` · ${LANGUAGE_NAMES[status.language]}` : ""}`
                  : "Waiting for a text field.")}
            </div>
          </div>
          <div className="row-control">
            {pausedUntil ? (
              <button type="button" className="btn primary" onClick={() => pause(null)}>
                Resume
              </button>
            ) : (
              <>
                <button type="button" className="btn" onClick={() => pause(15)}>
                  Pause 15 min
                </button>
                <button type="button" className="btn" onClick={() => pause(60)}>
                  Pause 1 hour
                </button>
              </>
            )}
          </div>
        </div>
        {pausedUntil ? (
          <div style={{ padding: "0 18px 16px" }}>
            <Note icon="pause">Paused until {formatClock(pausedUntil)}. Mote observes nothing while paused.</Note>
          </div>
        ) : null}
      </Card>

      <div className="section-label">Assistance</div>
      <Card>
        <ToggleRow
          title="Enable assistance"
          help="Master switch for inline completion, writing help and contextual suggestions. The command palette keeps working."
          checked={settings.general.assistanceEnabled}
          onChange={(v) => update((s) => void (s.general.assistanceEnabled = v))}
        />
        <ToggleRow
          title="Launch at login"
          help="Start Mote automatically when you sign in."
          checked={settings.general.launchAtLogin}
          onChange={(v) => update((s) => void (s.general.launchAtLogin = v))}
        />
        <Row title="Appearance" help="Follow the system, or always use light or dark.">
          <Segmented<Theme>
            label="Appearance"
            value={settings.general.theme}
            options={[
              { value: "system", label: "System" },
              { value: "light", label: "Light" },
              { value: "dark", label: "Dark" },
            ]}
            onChange={(theme) => update((s) => void (s.general.theme = theme))}
          />
        </Row>
      </Card>
      {error && !error.fields.length ? (
        <div style={{ marginTop: 12 }}>
          <Note tone="danger" icon="alert">
            {error.message}
          </Note>
        </div>
      ) : null}
      <Toast message={toast} />
    </Page>
  );
}
