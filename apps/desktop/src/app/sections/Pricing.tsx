import { useEffect, useState } from "react";

import type { ModelPricing } from "../../bindings/ModelPricing";
import { Card, Note } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { api, errorMessage } from "../../lib/ipc";

export function PricingEditor() {
  const [rows, setRows] = useState<ModelPricing[]>([]);
  const [draft, setDraft] = useState({ model: "", input: "", output: "", date: new Date().toISOString().slice(0, 10) });
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    api.listPricing().then(setRows, (e) => setProblem(errorMessage(e)));
  }, []);
  const save = async () => {
    try {
      setRows(
        await api.savePricing({
          id: null,
          provider: "groq",
          model: draft.model.trim(),
          inputCostPerMillion: Number(draft.input),
          outputCostPerMillion: Number(draft.output),
          effectiveDate: draft.date,
          source: "user",
        }),
      );
      setDraft({ ...draft, model: "", input: "", output: "" });
      setProblem(null);
    } catch (e) {
      setProblem(errorMessage(e));
    }
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
                    <button type="button" className="btn ghost small" aria-label={`Delete price for ${r.model}`} onClick={() => api.deletePricing(r.id ?? 0).then(setRows)}>
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
          <button type="button" className="btn ghost" onClick={() => api.resetPricing().then(setRows)}>
            Reset to built-in
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
