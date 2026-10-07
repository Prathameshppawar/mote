import { useCallback, useEffect, useState } from "react";

import type { ModelAssignments } from "../../bindings/ModelAssignments";
import type { ModelOption } from "../../bindings/ModelOption";
import { Card, Note, Page, Row } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { api, errorMessage } from "../../lib/ipc";
import { useSettings } from "../settingsContext";
import { PricingEditor } from "./Pricing";

const ROLES: { key: keyof ModelAssignments; title: string; help: string }[] = [
  { key: "completion", title: "Completion", help: "Inline continuations. Latency first: a fast model with reasoning off." },
  { key: "classification", title: "Classification", help: "Decides conversation vs prompt when local signals are unsure." },
  { key: "writing", title: "Writing", help: "Grammar fixes, rewrites, tone and translation." },
  { key: "reasoning", title: "Reasoning", help: "Prompt enhancement and analysis of copied content. Quality first." },
  { key: "fallback", title: "Fallback", help: "Used automatically if a role's model is withdrawn or unavailable." },
];

function price(m: ModelOption): string {
  if (m.inputCostPerMillion === null || m.outputCostPerMillion === null) return "price unknown";
  return `$${m.inputCostPerMillion} in · $${m.outputCostPerMillion} out per 1M`;
}

export function Models() {
  const { settings, update, fieldError } = useSettings();
  const [models, setModels] = useState<ModelOption[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async (refresh: boolean) => {
    setLoading(true);
    try {
      setModels(await api.listModels(refresh));
      setProblem(null);
    } catch (e) {
      setProblem(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load(false);
  }, [load]);

  if (!settings) return null;
  const assigned = settings.provider.groq.models;
  const known = new Set(models?.map((m) => m.id) ?? []);

  return (
    <Page title="Models" subtitle="Each kind of work uses its own model, so completion stays fast while prompt enhancement stays thoughtful.">
      <Card
        title="Model assignments"
        subtitle="Models come from your Groq account. Prices are list prices used for estimates."
        actions={
          <button type="button" className="btn small" onClick={() => load(true)} disabled={loading}>
            {loading ? <span className="spinner" /> : <Icon name="refresh" size={14} />}
            Refresh
          </button>
        }
      >
        {ROLES.map((role) => {
          const current = assigned[role.key];
          const options = models ?? [];
          const missing = models !== null && !known.has(current);
          return (
            <Row
              key={role.key}
              title={role.title}
              help={fieldError(`provider.groq.models.${role.key}`) ?? (missing ? `“${current}” is not available for this key.` : role.help)}
            >
              <select
                className="select"
                aria-label={`${role.title} model`}
                value={current}
                aria-invalid={missing || undefined}
                onChange={(e) => update((s) => void (s.provider.groq.models[role.key] = e.target.value))}
                style={{ width: 300 }}
              >
                {!known.has(current) ? <option value={current}>{current}</option> : null}
                {options.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.id} — {price(m)}
                    {m.recommendedFor.includes(role.key === "fallback" ? "completion" : role.key) ? " (recommended)" : ""}
                  </option>
                ))}
              </select>
            </Row>
          );
        })}
      </Card>
      {problem ? (
        <div style={{ marginTop: 12 }}>
          <Note tone="danger" icon="alert">
            {problem} The current assignments still work if those models exist.
          </Note>
        </div>
      ) : null}
      <div style={{ marginTop: 16 }}>
        <Note>
          Reasoning models such as GPT-OSS always spend some hidden reasoning tokens; Mote reserves room for them automatically. Qwen runs with reasoning turned off for completion, which keeps suggestions under half a second on Groq.
        </Note>
      </div>
      <PricingEditor />
    </Page>
  );
}
