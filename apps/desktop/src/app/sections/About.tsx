import { useEffect, useState } from "react";

import type { AppInfoResponse } from "../../bindings/AppInfoResponse";
import type { UpdateStatus } from "../../bindings/UpdateStatus";
import { Card, Page, Row } from "../../components/controls";
import { Logo } from "../../components/Icon";
import { api, errorMessage, subscribe } from "../../lib/ipc";
import { openLink } from "../../lib/links";

function updateText(status: UpdateStatus | null): string {
  switch (status?.state.state) {
    case undefined:
    case "idle":
      return "Mote checks for updates in the background.";
    case "checking":
      return "Checking for updates…";
    case "up_to_date":
      return "Mote is up to date.";
    case "downloading":
      return `Downloading version ${status.state.version}…`;
    case "ready":
      return `Version ${status.state.version} is downloaded and verified. Restart to finish updating.`;
    case "failed":
      return status.state.message;
  }
}

/** Update status, a manual check, and "Restart to update" once a version is ready. */
function Updates() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    api.updateStatus().then(setStatus, () => undefined);
    const unlisten = subscribe<UpdateStatus>("update-status", setStatus);
    return () => void unlisten.then((fn) => fn());
  }, []);
  const busy = status?.state.state === "checking" || status?.state.state === "downloading";
  const ready = status?.state.state === "ready";
  const run = async (action: () => Promise<unknown>) => {
    try {
      setProblem(null);
      await action();
    } catch (e) {
      setProblem(errorMessage(e));
    }
  };
  return (
    <Card title="Updates">
      <Row title={`Version ${status?.currentVersion ?? ""}`} help={problem ?? updateText(status)}>
        {ready ? (
          <button type="button" className="btn primary" onClick={() => run(api.installUpdate)}>
            Restart to update
          </button>
        ) : (
          <button type="button" className="btn" disabled={busy} onClick={() => run(async () => setStatus(await api.checkForUpdates()))}>
            {busy ? <span className="spinner" aria-hidden="true" /> : null}
            Check for updates
          </button>
        )}
      </Row>
    </Card>
  );
}

export function About() {
  const [info, setInfo] = useState<AppInfoResponse | null>(null);
  useEffect(() => {
    api.appInfo().then(setInfo, () => undefined);
  }, []);
  return (
    <Page title="About">
      <Card>
        <div className="card-body" style={{ display: "flex", gap: 18, alignItems: "center", padding: 22 }}>
          <Logo size={64} />
          <div>
            <div style={{ fontSize: 20, fontWeight: 660, letterSpacing: "-0.01em" }}>Mote {info?.version}</div>
            <div className="muted">AI that stays in the flow.</div>
            <div className="muted" style={{ fontSize: 12.5, marginTop: 4 }}>
              {info ? `${info.platform === "macos" ? "macOS" : info.platform === "windows" ? "Windows" : info.platform} · ${info.arch} · ${info.identifier}` : null}
            </div>
          </div>
        </div>
      </Card>
      <Updates />
      <Card title="Mote">
        <div className="card-body stack">
          <p className="muted">
            A local-first, context-aware writing and prompting layer for macOS and Windows. Mote completes sentences, fixes spelling and grammar,
            enhances AI prompts and understands Hinglish and romanized Marathi without leaving the app you are writing in.
          </p>
          <p className="muted">
            No accounts, no servers, no telemetry. Settings, usage statistics and activity metadata live in a local database; your API key lives in the
            system keychain. Update checks download the latest release manifest from GitHub and send nothing about you.
          </p>
          <p>
            <a href="https://github.com/Prathameshppawar/mote" onClick={openLink}>
              Source code
            </a>{" "}
            ·{" "}
            <a href="https://github.com/Prathameshppawar/mote/blob/main/docs/privacy/privacy-model.md" onClick={openLink}>
              Privacy model
            </a>{" "}
            ·{" "}
            <a href="https://github.com/Prathameshppawar/mote/issues" onClick={openLink}>
              Report an issue
            </a>
          </p>
        </div>
      </Card>
      <Card title="Credits">
        <div className="card-body stack">
          <p className="muted">© 2026 Prathamesh Pawar. Released under the MIT License.</p>
          <p className="muted">
            English word frequencies from SymSpell&apos;s frequency dictionary (MIT, © Wolf Garbe). Built with Tauri, Rust, React and SQLite. AI
            inference by Groq.
          </p>
        </div>
      </Card>
    </Page>
  );
}
