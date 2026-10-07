import { useCallback, useEffect, useState } from "react";

import type { Diagnostics as DiagnosticsData } from "../../bindings/Diagnostics";
import { Card, Note, Page, Toast, } from "../../components/controls";
import { useToast } from "../../components/useToast";
import { Icon } from "../../components/Icon";
import { formatCount, formatLatency, formatRelative } from "../../lib/format";
import { api, errorMessage } from "../../lib/ipc";
import { STATE_TEXT } from "../engineStatus";

export function Diagnostics() {
  const [data, setData] = useState<DiagnosticsData | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [toast, showToast] = useToast();
  const load = useCallback(() => {
    api.diagnostics().then(
      (d) => {
        setData(d);
        setProblem(null);
      },
      (e) => setProblem(errorMessage(e)),
    );
  }, []);
  useEffect(load, [load]);

  const copy = async () => {
    try {
      await api.copyDiagnostics();
      showToast("Diagnostics copied (no keys, text or app names)");
    } catch (e) {
      showToast(errorMessage(e));
    }
  };

  return (
    <Page title="Diagnostics" subtitle="Health of each part of Mote. Copying diagnostics never includes your API key, typed text, clipboard, prompts or app names.">
      <div style={{ display: "flex", gap: 8, marginBottom: 16 }}>
        <button type="button" className="btn primary" onClick={copy} disabled={!data}>
          <Icon name="copy" />
          Copy diagnostics
        </button>
        <button type="button" className="btn" onClick={load}>
          <Icon name="refresh" />
          Refresh
        </button>
      </div>
      {problem ? (
        <Note tone="danger" icon="alert">
          {problem}
        </Note>
      ) : null}
      {data ? (
        <>
          <Card title="Mote">
            <dl className="dl" style={{ marginTop: 8 }}>
              <dt>Version</dt>
              <dd>{data.version}</dd>
              <dt>Operating system</dt>
              <dd>
                {data.os} ({data.arch})
              </dd>
              <dt>Context engine</dt>
              <dd>
                {STATE_TEXT[data.engine.state]}
                {data.engine.message ? ` · ${data.engine.message}` : ""}
              </dd>
              <dt>Accessibility</dt>
              <dd>{data.permissions.accessibility === "not_required" ? "Not required on this OS" : data.permissions.accessibility}</dd>
              <dt>Secure input</dt>
              <dd>{data.permissions.secureInputActive ? "Active (Mote pauses)" : "Inactive"}</dd>
              <dt>Text observation</dt>
              <dd>{data.textObservation ? "On" : "Off"}</dd>
              <dt>Clipboard observation</dt>
              <dd>{data.clipboardObservation ? "On" : "Off"}</dd>
            </dl>
          </Card>
          <Card title="Provider">
            <dl className="dl" style={{ marginTop: 8 }}>
              <dt>Provider</dt>
              <dd>
                {data.provider} · <span className="mono">{data.baseUrl}</span>
              </dd>
              <dt>API key</dt>
              <dd>{data.hasApiKey ? "Configured (in keychain)" : "Missing"}</dd>
              <dt>Cloud AI</dt>
              <dd>{data.cloudAiEnabled ? "Enabled" : "Disabled in Privacy"}</dd>
              <dt>Health</dt>
              <dd>
                {data.providerHealth
                  ? data.providerHealth.ok
                    ? `OK · ${data.providerHealth.modelsAvailable} models · checked ${formatRelative(data.providerHealth.checkedAt)}`
                    : data.providerHealth.message
                  : "Not checked yet"}
              </dd>
              <dt>Models</dt>
              <dd className="mono">
                completion {data.models.completion}
                <br />
                writing {data.models.writing}
                <br />
                reasoning {data.models.reasoning}
                <br />
                fallback {data.models.fallback}
              </dd>
              <dt>Unavailable models</dt>
              <dd>{data.unavailableModels.length ? data.unavailableModels.join(", ") : "None"}</dd>
              <dt>Avg completion latency (24 h)</dt>
              <dd>
                {formatLatency(data.avgCompletionLatencyMs)} over {formatCount(data.completionRequests24h)} requests
              </dd>
              <dt>Last request</dt>
              <dd>
                {data.lastRequest
                  ? `${data.lastRequest.status} · ${data.lastRequest.model} · ${formatLatency(data.lastRequest.latencyMs)} · ${formatRelative(data.lastRequest.at)}${data.lastRequest.errorKind ? ` · ${data.lastRequest.errorKind}` : ""}`
                  : "None yet"}
              </dd>
            </dl>
          </Card>
          <Card title="Storage">
            <dl className="dl" style={{ marginTop: 8 }}>
              <dt>Database</dt>
              <dd>
                {data.databaseOk ? "Healthy" : "Integrity check failed"}
                {data.database ? ` · schema v${data.database.schemaVersion} · ${Math.round((data.database.sizeBytes ?? 0) / 1024)} KB` : ""}
              </dd>
              <dt>Records</dt>
              <dd>
                {data.database
                  ? `${formatCount(data.database.usageEvents)} usage events · ${formatCount(data.database.contextEvents)} activity events · ${data.database.exclusions} exclusions`
                  : "—"}
              </dd>
              <dt>Database file</dt>
              <dd className="mono">{data.databasePath}</dd>
              <dt>Logs</dt>
              <dd className="mono">{data.logDir}</dd>
            </dl>
          </Card>
        </>
      ) : null}
      <Toast message={toast} />
    </Page>
  );
}
