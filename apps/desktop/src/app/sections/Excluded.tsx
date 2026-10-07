import { useEffect, useState } from "react";

import type { AppInfo } from "../../bindings/AppInfo";
import type { ExclusionKind } from "../../bindings/ExclusionKind";
import type { ExclusionRule } from "../../bindings/ExclusionRule";
import { Card, Note, Page, Segmented } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { api, errorMessage } from "../../lib/ipc";

export function ExcludedApps() {
  const [rules, setRules] = useState<ExclusionRule[]>([]);
  const [always, setAlways] = useState<string[]>([]);
  const [apps, setApps] = useState<AppInfo[]>([]);
  const [kind, setKind] = useState<ExclusionKind>("app");
  const [pattern, setPattern] = useState("");
  // Choosing a running app only fills in the field; Add confirms. (Arrow keys on a
  // closed select fire change events on Windows.)
  const [picked, setPicked] = useState<AppInfo | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    api.listExclusions().then(setRules, (e) => setProblem(errorMessage(e)));
    api.alwaysExcluded().then(setAlways, () => undefined);
    api.runningApps().then(setApps, () => undefined);
  }, []);

  const add = async (k: ExclusionKind, value: string, name: string) => {
    if (!value.trim()) return;
    try {
      setRules(await api.addExclusion(k, value.trim(), name.trim()));
      setPattern("");
      setPicked(null);
      setProblem(null);
    } catch (e) {
      setProblem(errorMessage(e));
    }
  };

  /** The picked app's display name, or what was typed. */
  const appName = () => (picked && picked.id === pattern.trim() ? picked.name : pattern);

  const excludedIds = new Set(rules.filter((r) => r.kind === "app").map((r) => r.pattern.toLowerCase()));
  const candidates = apps.filter((a) => !excludedIds.has(a.id.toLowerCase()));

  return (
    <Page title="Excluded Apps" subtitle="Mote never reads text or clipboard content from these apps or windows, and never sends anything from them anywhere.">
      <Card title="Always excluded">
        <ul className="list" style={{ marginTop: 8 }}>
          {always.map((a) => (
            <li key={a} className="list-item">
              <span style={{ display: "flex", gap: 10, alignItems: "center" }}>
                <Icon name="lock" />
                {a}
              </span>
            </li>
          ))}
        </ul>
      </Card>

      <div className="section-label">Your exclusions</div>
      <Card>
        {rules.length ? (
          <ul className="list">
            {rules.map((r) => (
              <li key={r.id} className="list-item">
                <span>
                  <strong style={{ fontWeight: 560 }}>{r.displayName}</strong>{" "}
                  <span className="muted">
                    {r.kind === "app" ? `app · ${r.pattern}` : `window title contains “${r.pattern}”`}
                  </span>
                </span>
                <button
                  type="button"
                  className="btn ghost small"
                  aria-label={`Remove ${r.displayName}`}
                  onClick={() => api.removeExclusion(r.id).then(setRules, (e) => setProblem(errorMessage(e)))}
                >
                  <Icon name="trash" size={14} />
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="card-body muted">No exclusions yet. Add banking apps, private notes, or anything else Mote should never see.</p>
        )}
      </Card>

      <div className="section-label">Add an exclusion</div>
      <Card>
        <div className="card-body stack">
          <Segmented<ExclusionKind>
            label="Exclusion type"
            value={kind}
            onChange={setKind}
            options={[
              { value: "app", label: "Application" },
              { value: "window_title", label: "Window title" },
            ]}
          />
          {kind === "app" ? (
            <div style={{ display: "flex", gap: 8 }}>
              <select
                className="select"
                aria-label="Running application"
                value={picked && picked.id === pattern ? picked.id : ""}
                onChange={(e) => {
                  const app = candidates.find((a) => a.id === e.target.value);
                  if (app) {
                    setPicked(app);
                    setPattern(app.id);
                  }
                }}
                style={{ flex: 1 }}
              >
                <option value="" disabled>
                  Choose a running app…
                </option>
                {candidates.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
              <input
                className="input mono"
                aria-label="Bundle identifier or executable"
                placeholder="or type com.example.app / app.exe"
                value={pattern}
                onChange={(e) => setPattern(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && add("app", pattern, appName())}
                style={{ flex: 1 }}
              />
              <button type="button" className="btn" disabled={!pattern.trim()} onClick={() => add("app", pattern, appName())}>
                Add
              </button>
            </div>
          ) : (
            <div style={{ display: "flex", gap: 8 }}>
              <input
                className="input"
                aria-label="Window title keyword"
                placeholder="e.g. NetBanking, Private Journal"
                value={pattern}
                onChange={(e) => setPattern(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && add("window_title", pattern, pattern)}
                style={{ flex: 1 }}
              />
              <button type="button" className="btn" disabled={!pattern.trim()} onClick={() => add("window_title", pattern, pattern)}>
                Add
              </button>
            </div>
          )}
          <p className="muted" style={{ fontSize: 12.5 }}>
            Window-title rules are useful for websites: a browser tab whose title contains the keyword is ignored while it is in front.
          </p>
        </div>
      </Card>
      {problem ? (
        <div style={{ marginTop: 12 }}>
          <Note tone="danger" icon="alert">
            {problem}
          </Note>
        </div>
      ) : null}
    </Page>
  );
}
