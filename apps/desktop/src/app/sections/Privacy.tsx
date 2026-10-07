import { useCallback, useEffect, useState } from "react";

import type { ContextEvent } from "../../bindings/ContextEvent";
import type { RetentionPeriod } from "../../bindings/RetentionPeriod";
import { Card, Note, Page, Row, Toast, ToggleRow, } from "../../components/controls";
import { useToast } from "../../components/useToast";
import { Icon } from "../../components/Icon";
import { describeEvent } from "../../lib/activity";
import { formatRelative } from "../../lib/format";
import { api, errorMessage } from "../../lib/ipc";
import { useSettings } from "../settingsContext";

function ConfirmButton({
  label,
  confirm,
  onConfirm,
  onError,
}: {
  label: string;
  confirm: string;
  onConfirm: () => Promise<void>;
  onError: (error: unknown) => void;
}) {
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(t);
  }, [armed]);
  return (
    <button
      type="button"
      className={`btn ${armed ? "primary" : "danger"}`}
      disabled={busy}
      onClick={async () => {
        if (!armed) return setArmed(true);
        setBusy(true);
        try {
          await onConfirm();
        } catch (e) {
          onError(e);
        } finally {
          setBusy(false);
          setArmed(false);
        }
      }}
    >
      {busy ? <span className="spinner" aria-hidden="true" /> : null}
      {armed ? confirm : label}
    </button>
  );
}

export function Privacy() {
  const { settings, update, replace } = useSettings();
  const [events, setEvents] = useState<ContextEvent[]>([]);
  const [toast, showToast] = useToast();

  const loadEvents = useCallback(() => {
    api.recentActivity(80).then(setEvents, () => setEvents([]));
  }, []);
  useEffect(loadEvents, [loadEvents]);

  if (!settings) return null;
  const p = settings.privacy;

  return (
    <Page title="Privacy" subtitle="Mote is local-first. Here is exactly what it observes, what can leave your computer, and how long anything is kept.">
      <Card title="What Mote observes">
        <ToggleRow
          title="Active application"
          help="Which app and window is in front. Required for exclusions to work, so turning it off pauses all ambient assistance."
          checked={p.observeApplications}
          onChange={(v) => update((s) => void (s.privacy.observeApplications = v))}
        />
        <ToggleRow
          title="Text near the cursor"
          help="Up to ~2,000 characters before the cursor in the focused field, held in memory (more, once, when you open the command palette). Never password fields, never excluded apps."
          checked={p.observeText}
          onChange={(v) => update((s) => void (s.privacy.observeText = v))}
        />
        <ToggleRow
          title="Clipboard"
          help="Copied text stays in memory for a few minutes for contextual suggestions. Content marked as concealed by password managers is never read."
          checked={p.observeClipboard}
          onChange={(v) => update((s) => void (s.privacy.observeClipboard = v))}
        />
      </Card>

      <div className="section-label">What can leave your computer</div>
      <Card>
        <ToggleRow
          title="Cloud AI (Groq)"
          help="Off means nothing is ever sent: completions, grammar checks and palette actions stop working, local spelling still works."
          checked={p.cloudAiEnabled}
          onChange={(v) => update((s) => void (s.privacy.cloudAiEnabled = v))}
        />
        <div style={{ padding: "0 18px 16px" }}>
          <Note icon="lock">
            Only the text a feature needs is sent, when that feature runs: the last ~600 characters for a completion, the finished sentence for a grammar
            check, and the selection, field or copied text you pick in the command palette. Mote has no servers, accounts or telemetry.
          </Note>
        </div>
      </Card>

      <div className="section-label">What is stored on disk</div>
      <Card>
        <ToggleRow
          title="Usage statistics"
          help="Token counts, latency and outcomes per request, for the Usage dashboard. Never prompts or responses."
          checked={settings.usage.analyticsEnabled}
          onChange={(v) => update((s) => void (s.usage.analyticsEnabled = v))}
        />
        <Row title="Activity history" help="App switches and suggestion outcomes (metadata only) shown in the log below.">
          <select
            className="select"
            aria-label="Keep activity history"
            value={p.contextRetention}
            onChange={(e) => update((s) => void (s.privacy.contextRetention = e.target.value as RetentionPeriod))}
          >
            <option value="off">Don&apos;t keep (memory only)</option>
            <option value="one_hour">1 hour</option>
            <option value="one_day">1 day</option>
            <option value="one_week">1 week</option>
          </select>
        </Row>
        <Row title="Usage history" help="How long request statistics are kept.">
          <select
            className="select"
            aria-label="Keep usage history"
            value={settings.usage.retentionDays}
            onChange={(e) => update((s) => void (s.usage.retentionDays = Number(e.target.value)))}
          >
            {[30, 90, 180, 365].map((d) => (
              <option key={d} value={d}>
                {d} days
              </option>
            ))}
          </select>
        </Row>
      </Card>

      <div className="section-label">Your data</div>
      <Card>
        <Row title="Clear activity" help="Deletes the activity log and forgets copied text held in memory.">
          <ConfirmButton
            label="Clear"
            confirm="Click to confirm"
            onConfirm={async () => {
              await api.clearContext();
              loadEvents();
              showToast("Activity cleared");
            }}
            onError={(e) => showToast(errorMessage(e))}
          />
        </Row>
        <Row title="Clear usage history" help="Deletes all usage statistics. Groq's own records are not affected.">
          <ConfirmButton
            label="Clear"
            confirm="Click to confirm"
            onConfirm={async () => {
              const n = await api.clearUsageHistory();
              showToast(`Removed ${n} records`);
            }}
            onError={(e) => showToast(errorMessage(e))}
          />
        </Row>
        <Row title="Reset Mote" help="Deletes all local data, settings and the stored API key, then restarts onboarding.">
          <ConfirmButton
            label="Reset everything"
            confirm="Click again to reset"
            onConfirm={async () => {
              replace(await api.resetLocalData());
            }}
            onError={(e) => showToast(errorMessage(e))}
          />
        </Row>
      </Card>

      <div className="section-label" id="activity">
        Recent activity
      </div>
      <Card
        subtitle="Exactly what Mote recorded. Content is never stored, only metadata."
        actions={
          <button type="button" className="btn small" onClick={loadEvents}>
            <Icon name="refresh" size={14} />
            Refresh
          </button>
        }
      >
        {events.length ? (
          <ul className="list" style={{ marginTop: 8 }}>
            {events.map((e, i) => (
              <li key={`${e.timestamp}-${i}`} className="list-item">
                <span>{describeEvent(e)}</span>
                <span className="muted" style={{ fontSize: 12 }}>
                  {formatRelative(e.timestamp)}
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="card-body muted">{p.contextRetention === "off" ? "History is off; nothing is written to disk." : "No activity recorded yet."}</p>
        )}
      </Card>
      <Toast message={toast} />
    </Page>
  );
}
