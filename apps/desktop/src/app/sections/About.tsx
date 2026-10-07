import { useEffect, useState } from "react";

import type { AppInfoResponse } from "../../bindings/AppInfoResponse";
import { Card, Page } from "../../components/controls";
import { Logo } from "../../components/Icon";
import { api } from "../../lib/ipc";
import { openLink } from "../../lib/links";

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
      <Card title="Mote">
        <div className="card-body stack">
          <p className="muted">
            A local-first, context-aware writing and prompting layer for macOS and Windows. Mote completes sentences, fixes spelling and grammar,
            enhances AI prompts and understands Hinglish and romanized Marathi without leaving the app you are writing in.
          </p>
          <p className="muted">
            No accounts, no servers, no telemetry. Settings, usage statistics and activity metadata live in a local database; your API key lives in the
            system keychain.
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
