import { useEffect, useState } from "react";

import type { HealthReport } from "../../bindings/HealthReport";
import type { ProviderStatus } from "../../bindings/ProviderStatus";
import { Meter } from "../../charts/figures";
import { Card, CommitInput, Note, Page, Row } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { formatCount, formatLatency, formatSeconds } from "../../lib/format";
import { api, errorMessage } from "../../lib/ipc";
import { openLink } from "../../lib/links";
import { useSettings } from "../settingsContext";

export function HealthLine({ report }: { report: HealthReport }) {
  return report.ok ? (
    <Note icon="check">
      Connected. {report.modelsAvailable} chat models available
      {report.latencyMs !== null ? ` · ${formatLatency(report.latencyMs)}` : ""}.
    </Note>
  ) : (
    <Note tone="danger" icon="alert">
      {report.message ?? "Mote could not connect to Groq."}
    </Note>
  );
}

export function Providers() {
  const { settings, update, fieldError } = useSettings();
  const [status, setStatus] = useState<ProviderStatus | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState<"save" | "test" | "remove" | null>(null);
  const [report, setReport] = useState<HealthReport | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    api.providerStatus().then(setStatus, (e) => setProblem(errorMessage(e)));
  }, []);

  if (!settings) return null;
  const groq = settings.provider.groq;
  const limits = status?.limits;

  const save = async () => {
    setBusy("save");
    setProblem(null);
    try {
      const next = await api.setApiKey(key);
      setStatus(next);
      setReport(next.health);
      setKey("");
    } catch (e) {
      setProblem(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const test = async () => {
    setBusy("test");
    setProblem(null);
    try {
      setReport(await api.testConnection(key || undefined));
    } catch (e) {
      setProblem(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const remove = async () => {
    setBusy("remove");
    try {
      setStatus(await api.clearApiKey());
      setReport(null);
    } catch (e) {
      setProblem(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Page title="AI Providers" subtitle="Mote v1 uses Groq for fast, low-cost inference. Your key stays in the system keychain.">
      <Card
        title="Groq"
        subtitle="OpenAI-compatible API at api.groq.com"
        actions={<span className="badge" data-tone={status?.hasApiKey ? "good" : undefined}>{status?.hasApiKey ? "Key saved" : "No key"}</span>}
      >
        <Row
          title="API key"
          help={
            status?.hasApiKey ? (
              <>
                Saved in the {navigator.userAgent.includes("Windows") ? "Windows Credential Manager" : "macOS Keychain"} ({status.keyHint}). Paste a new key to replace it.
              </>
            ) : (
              <>
                Create a free key in the{" "}
                <a href="https://console.groq.com/keys" onClick={openLink}>
                  Groq console
                </a>
                .
              </>
            )
          }
        >
          <input
            className="input mono"
            type="password"
            autoComplete="off"
            spellCheck={false}
            placeholder={status?.hasApiKey ? "•••••••••••••••• (saved)" : "gsk_…"}
            aria-label="Groq API key"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && key && save()}
            style={{ width: 240 }}
          />
          <button type="button" className="btn primary" disabled={!key || busy !== null} onClick={save}>
            {busy === "save" ? <span className="spinner" /> : null}
            Save
          </button>
        </Row>
        <Row title="Connection" help="Checks the key and that the configured models are available. Uses no tokens.">
          <button type="button" className="btn" disabled={busy !== null || (!key && !status?.hasApiKey)} onClick={test}>
            {busy === "test" ? <span className="spinner" /> : <Icon name="refresh" />}
            Test connection
          </button>
          {status?.hasApiKey ? (
            <button type="button" className="btn ghost danger" disabled={busy !== null} onClick={remove}>
              Remove key
            </button>
          ) : null}
        </Row>
        {report || problem ? (
          <div style={{ padding: "0 18px 16px" }}>
            {problem ? (
              <Note tone="danger" icon="alert">
                {problem}
              </Note>
            ) : report ? (
              <HealthLine report={report} />
            ) : null}
          </div>
        ) : null}
      </Card>

      <div className="section-label">Provider usage (reported by Groq)</div>
      <Card subtitle={undefined}>
        <div className="card-body">
          {limits && limits.requestsLimit && limits.tokensLimit ? (
            <>
              <Meter
                label="Requests today"
                used={limits.requestsLimit - (limits.requestsRemaining ?? limits.requestsLimit)}
                limit={limits.requestsLimit}
                detail={`${formatCount(limits.requestsRemaining ?? 0)} of ${formatCount(limits.requestsLimit)} left · resets in ${formatSeconds(limits.requestsResetSecs)}`}
              />
              <Meter
                label="Tokens this minute"
                used={limits.tokensLimit - (limits.tokensRemaining ?? limits.tokensLimit)}
                limit={limits.tokensLimit}
                detail={`${formatCount(limits.tokensRemaining ?? 0)} of ${formatCount(limits.tokensLimit)} left · resets in ${formatSeconds(limits.tokensResetSecs)}`}
              />
              <p className="muted" style={{ fontSize: 12, marginTop: 14 }}>
                Limits come from Groq&apos;s response headers on the most recent request. Billing and invoices live in the{" "}
                <a href="https://console.groq.com/settings/billing" onClick={openLink}>
                  Groq console
                </a>
                , which is the source of truth for cost.
              </p>
            </>
          ) : (
            <p className="muted">Groq reports your rate limits with each response. They appear here after the first request.</p>
          )}
        </div>
      </Card>

      <div className="section-label">Advanced</div>
      <Card>
        <Row title="API base URL" help={fieldError("provider.groq.baseUrl") ?? "Change only for a compatible proxy. Must be https."}>
          <CommitInput
            label="API base URL"
            className="mono"
            width={300}
            value={groq.baseUrl}
            invalid={Boolean(fieldError("provider.groq.baseUrl"))}
            onCommit={(v) => update((s) => void (s.provider.groq.baseUrl = v))}
          />
        </Row>
        <Row title="Request timeout" help="Interactive actions wait at most this long for each attempt.">
          <select
            className="select"
            aria-label="Request timeout"
            value={groq.requestTimeoutMs}
            onChange={(e) => update((s) => void (s.provider.groq.requestTimeoutMs = Number(e.target.value)))}
          >
            {[10_000, 20_000, 30_000, 60_000].map((ms) => (
              <option key={ms} value={ms}>
                {ms / 1000} seconds
              </option>
            ))}
          </select>
        </Row>
      </Card>
      <div style={{ marginTop: 16 }}>
        <Note icon="lock">
          Future versions add local providers (Ollama, LM Studio) behind the same interface. Nothing needs to be installed for Groq.
        </Note>
      </div>
    </Page>
  );
}
