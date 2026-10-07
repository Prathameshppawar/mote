import { useEffect, useState } from "react";

import type { ModelPricing } from "../../bindings/ModelPricing";
import { Card, Note } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { parsePrice } from "../../lib/format";
import { api, errorMessage } from "../../lib/ipc";

/** Today's date in the user's time zone, as YYYY-MM-DD. */
function localDate(now = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

export function PricingEditor() {
  const [rows, setRows] = useState<ModelPricing[]>([]);
  const [draft, setDraft] = useState({ model: "", input: "", output: "", date: localDate() });
  const [problem, setProblem] = useState<string | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  useEffect(() => {
    api.listPricing().then(setRows, (e) => setProblem(errorMessage(e)));
  }, []);
  useEffect(() => {
    if (!confirmReset) return;
    const t = setTimeout(() => setConfirmReset(false), 4000);
    return () => clearTimeout(t);
  }, [confirmReset]);
  const run = async (action: () => Promise<ModelPricing[]>) => {
    try {
      setRows(await action());
      setProblem(null);
      return true;
    } catch (e) {
      setProblem(errorMessage(e));
      return false;
    }
  };
  const save = async () => {
    const input = parsePrice(draft.input);
    const output = parsePrice(draft.output);
    if (input === null || output === null) {
      setProblem("Prices are numbers of US dollars per million tokens, such as 0.15.");
      return;
    }
    const saved = await run(() =>
      api.savePricing({
        id: null,
        provider: "groq",
        model: draft.model.trim(),
        inputCostPerMillion: input,
        outputCostPerMillion: output,
        effectiveDate: draft.date,
        source: "user",
      }),
    );
    if (saved) setDraft({ ...draft, model: "", input: "", output: "" });
  };
  const reset = () => {
    if (!confirmReset) return setConfirmReset(true);
    setConfirmReset(false);
    void run(() => api.resetPricing());
  };
  return (
    <>
      <div className="section-label">Pricing</div>
      <Card subtitle="USD per million tokens. A new row applies from its effective date; earlier usage keeps its old price.">
        <table className="data-table" style={{ marginTop: 8 }}>
          <thead>
            <tr>
              <th scope="col">Model</th>
              <th scope="col">Input</th>
              <th scope="col">Output</th>
              <th scope="col">From</th>
              <th scope="col">Source</th>
              <th scope="col">
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.id ?? `${r.model}-${r.effectiveDate}-${r.source}`}>
                <td className="mono">{r.model}</td>
                <td>${r.inputCostPerMillion}</td>
                <td>${r.outputCostPerMillion}</td>
                <td>{r.effectiveDate}</td>
                <td>{r.source === "builtin" ? "Built-in" : "Yours"}</td>
                <td>
                  {r.source === "user" && r.id !== null ? (
                    <button type="button" className="btn ghost small" aria-label={`Delete price for ${r.model}`} onClick={() => void run(() => api.deletePricing(r.id ?? 0))}>
                      <Icon name="trash" size={14} />
                    </button>
                  ) : null}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="card-body" style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <input className="input mono" placeholder="model id" aria-label="Model" value={draft.model} onChange={(e) => setDraft({ ...draft, model: e.target.value })} style={{ flex: "1 1 180px" }} />
          <input className="input" placeholder="input $/1M" aria-label="Input price per million" inputMode="decimal" value={draft.input} onChange={(e) => setDraft({ ...draft, input: e.target.value })} style={{ width: 110 }} />
          <input className="input" placeholder="output $/1M" aria-label="Output price per million" inputMode="decimal" value={draft.output} onChange={(e) => setDraft({ ...draft, output: e.target.value })} style={{ width: 110 }} />
          <input className="input" type="date" aria-label="Effective date" value={draft.date} onChange={(e) => setDraft({ ...draft, date: e.target.value })} />
          <button type="button" className="btn" disabled={!draft.model || draft.input === "" || draft.output === ""} onClick={save}>
            Add price
          </button>
          <button type="button" className={`btn ${confirmReset ? "danger" : "ghost"}`} onClick={reset}>
            {confirmReset ? "Remove your prices?" : "Reset to built-in"}
          </button>
        </div>
        {problem ? (
          <div style={{ padding: "0 18px 16px" }}>
            <Note tone="danger" icon="alert">
              {problem}
            </Note>
          </div>
        ) : null}
      </Card>
    </>
  );
}
