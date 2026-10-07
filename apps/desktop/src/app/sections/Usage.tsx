import { useCallback, useEffect, useState } from "react";

import type { SeriesPoint } from "../../bindings/SeriesPoint";
import type { UsageResponse } from "../../bindings/UsageResponse";
import { BarList } from "../../charts/BarList";
import { ChartFrame } from "../../charts/ChartFrame";
import { Meter, StatTile } from "../../charts/figures";
import { TimeSeriesChart } from "../../charts/TimeSeriesChart";
import { Card, Note, Page, Segmented } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { FEATURE_GROUP_LABELS, FEATURE_LABELS, formatCost, formatCostEstimate, formatCount, formatDayLabel, formatLatency, formatPercent, formatSeconds, formatTokens } from "../../lib/format";
import { api, errorMessage, subscribe } from "../../lib/ipc";

type Range = "today" | "30d";

function series(points: SeriesPoint[], pick: (p: SeriesPoint) => number | null) {
  return points.map((p) => ({ label: p.label, value: pick(p) }));
}

export function Usage() {
  const [data, setData] = useState<UsageResponse | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [range, setRange] = useState<Range>("30d");
  const [refreshing, setRefreshing] = useState(false);

  const load = useCallback(async () => {
    setRefreshing(true);
    try {
      setData(await api.usage());
      setProblem(null);
    } catch (e) {
      setProblem(errorMessage(e));
    } finally {
      setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void load();
    const unlisten = subscribe("usage-updated", () => void load());
    return () => void unlisten.then((fn) => fn());
  }, [load]);

  if (!data) {
    return (
      <Page title="Usage" wide>
        {problem ? (
          <Note tone="danger" icon="alert">
            {problem}
          </Note>
        ) : (
          <span className="spinner" role="status" aria-label="Loading usage" />
        )}
      </Page>
    );
  }

  const d = data.dashboard;
  const today = range === "today";
  const breakdown = today ? d.breakdownToday : d.breakdown30d;
  const suggestions = today ? data.suggestionsToday : data.suggestions30d;
  const points = today ? d.hourly : d.daily;
  const formatLabel = today ? (l: string) => l : formatDayLabel;
  const rangeText = today ? "Today, by hour" : "Last 30 days, by day";
  const table = (pick: (p: SeriesPoint) => number | null, fmt: (v: number) => string) =>
    points.map((p) => {
      const v = pick(p);
      return { label: formatLabel(p.label), value: v === null ? "—" : fmt(v) };
    });
  const perf = breakdown.performance;
  const acceptance = suggestions.shown ? suggestions.accepted / suggestions.shown : 0;
  const limits = d.providerLimits;
  const fading = { opacity: refreshing ? 0.7 : 1, transition: "opacity 120ms" };

  return (
    <Page title="Usage" wide subtitle="Mote's own record of every model request, kept on this computer. Costs are estimates at list price.">
      {!data.analyticsEnabled ? (
        <div style={{ marginBottom: 16 }}>
          <Note>Usage statistics are off (Settings → Privacy). New requests aren&apos;t recorded.</Note>
        </div>
      ) : null}

      <div className="tile-row" aria-label="Usage summary" style={fading}>
        <StatTile
          hero
          label="Tokens today"
          value={formatTokens(d.today.totalTokens)}
          sub={`${formatCount(d.today.requests)} requests · ${formatCostEstimate(d.today.estimatedCostUsd, d.today.costComplete)} estimated`}
        />
        <StatTile
          label="This week"
          value={formatTokens(d.week.totalTokens)}
          sub={`${formatCount(d.week.requests)} requests · ${formatCostEstimate(d.week.estimatedCostUsd, d.week.costComplete)} estimated`}
        />
        <StatTile
          label="This month"
          value={formatTokens(d.month.totalTokens)}
          sub={`${formatCount(d.month.requests)} requests · ${formatCostEstimate(d.month.estimatedCostUsd, d.month.costComplete)} estimated`}
        />
        <StatTile label="Avg latency today" value={formatLatency(d.today.avgLatencyMs)} sub={`error rate ${formatPercent(d.today.errorRate)}`} />
      </div>

      <div className="section-label">Mote usage vs. provider usage</div>
      <div className="chart-grid-2">
        <Card title="Groq rate limits" subtitle="Reported by Groq on the latest response.">
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
              </>
            ) : (
              <p className="muted">Appears after Mote&apos;s first request in this session.</p>
            )}
          </div>
        </Card>
        <Card title="About these numbers">
          <div className="card-body stack">
            <p className="muted">
              <strong>Mote usage</strong> is counted here from the token counts Groq returns with each response, priced with Mote&apos;s pricing table
              (Models → Pricing). Free tiers and discounts are not modelled, and requests cancelled because you kept typing carry no token count.
            </p>
            <p className="muted">
              <strong>Provider usage</strong> is what Groq records and bills. The Groq console is the source of truth for cost.
            </p>
            <p className="muted">
              <Icon name="lock" size={13} /> Mote stores usage statistics locally. Raw prompt content is not required for usage analytics and is never stored.
            </p>
            {d.unpricedModels.length ? <p className="muted">No price is known for: {d.unpricedModels.join(", ")}.</p> : null}
          </div>
        </Card>
      </div>

      <div className="filter-row" role="toolbar" aria-label="Usage range">
        <Segmented<Range>
          label="Range"
          value={range}
          onChange={setRange}
          options={[
            { value: "today", label: "Today" },
            { value: "30d", label: "Last 30 days" },
          ]}
        />
        <span className="muted">
          {formatCount(breakdown.summary.requests)} requests · {formatTokens(breakdown.summary.totalTokens)} tokens · {formatCost(breakdown.summary.estimatedCostUsd)} estimated
        </span>
      </div>

      <div style={fading}>
        <div className="chart-grid-2">
          <ChartFrame title="Tokens" subtitle={rangeText} table={table((p) => p.totalTokens, formatTokens)} valueHeader="Tokens">
            <TimeSeriesChart variant="area" points={series(points, (p) => p.totalTokens)} format={formatTokens} formatLabel={formatLabel} ariaLabel={`Tokens. ${rangeText}`} integer />
          </ChartFrame>
          <ChartFrame title="Requests" subtitle={rangeText} table={table((p) => p.requests, formatCount)} valueHeader="Requests">
            <TimeSeriesChart variant="columns" points={series(points, (p) => p.requests)} format={formatCount} formatLabel={formatLabel} ariaLabel={`Requests. ${rangeText}`} integer />
          </ChartFrame>
          <ChartFrame title="Estimated cost" subtitle={`${rangeText} · list price`} table={table((p) => p.estimatedCostUsd, formatCost)} valueHeader="Est. cost">
            <TimeSeriesChart variant="line" points={series(points, (p) => p.estimatedCostUsd)} format={formatCost} formatLabel={formatLabel} ariaLabel={`Estimated cost. ${rangeText}`} />
          </ChartFrame>
          <ChartFrame title="Average latency" subtitle={rangeText} table={table((p) => p.avgLatencyMs, formatLatency)} valueHeader="Avg latency">
            <TimeSeriesChart variant="line" points={series(points, (p) => p.avgLatencyMs)} format={formatLatency} formatLabel={formatLabel} ariaLabel={`Average latency. ${rangeText}`} />
          </ChartFrame>
        </div>

        <div className="chart-grid-2" style={{ marginTop: 16 }}>
          <Card
            title="Requests by feature"
            subtitle={breakdown.topFeature ? `Top: ${FEATURE_GROUP_LABELS[breakdown.topFeature.group]} at ${formatPercent(breakdown.topFeature.share)}` : "No requests in this range"}
          >
            {breakdown.byFeature.length ? (
              <BarList
                ariaLabel="Requests by feature"
                items={breakdown.byFeature.map((f) => ({
                  key: f.feature,
                  label: FEATURE_LABELS[f.feature] ?? f.feature,
                  value: f.requests,
                  valueLabel: `${formatCount(f.requests)} · ${formatPercent(f.share)}`,
                  detail: `${formatTokens(f.totalTokens)} tokens · ${formatCost(f.estimatedCostUsd)} · ${formatLatency(f.avgLatencyMs)} avg`,
                }))}
              />
            ) : (
              <p className="card-body muted">Nothing yet. Type in any app and Mote&apos;s requests appear here.</p>
            )}
          </Card>
          <Card title="Performance" subtitle="End to end, including network.">
            <dl className="dl">
              <dt>Average latency</dt>
              <dd>
                {formatLatency(perf.avgLatencyMs)} · median {formatLatency(perf.medianLatencyMs)}
              </dd>
              <dt>Completion latency</dt>
              <dd>
                {formatLatency(perf.completionMedianLatencyMs)} median · {formatLatency(perf.completionAvgLatencyMs)} avg
              </dd>
              <dt>Classification latency</dt>
              <dd>
                {formatLatency(perf.classificationMedianLatencyMs)} median · {formatLatency(perf.classificationAvgLatencyMs)} avg
              </dd>
              <dt>Slowest 5%</dt>
              <dd>{perf.p95LatencyMs === null ? "—" : `${formatLatency(perf.p95LatencyMs)} or more`}</dd>
              <dt>Error rate</dt>
              <dd>
                {formatPercent(perf.errorRate)} · {formatCount(perf.failedRequests)} failed ({formatCount(perf.timeouts)} timeouts)
              </dd>
              <dt>Rate-limit events</dt>
              <dd>{formatCount(perf.rateLimitEvents)}</dd>
              <dt>Cancelled (you kept typing)</dt>
              <dd>{formatCount(perf.cancelled)}</dd>
              <dt>Suggestions accepted</dt>
              <dd>
                {formatCount(suggestions.accepted)} of {formatCount(suggestions.shown)} ({formatPercent(acceptance)})
              </dd>
            </dl>
          </Card>
        </div>

        <Card title="Model usage" subtitle={today ? "Today, per model." : "Last 30 days, per model."}>
          <div style={{ overflowX: "auto", marginTop: 8 }}>
            <table className="data-table">
              <thead>
                <tr>
                  <th scope="col">Model</th>
                  <th scope="col">Requests</th>
                  <th scope="col">Input tokens</th>
                  <th scope="col">Output tokens</th>
                  <th scope="col">Total tokens</th>
                  <th scope="col">Avg latency</th>
                  <th scope="col">Est. cost</th>
                </tr>
              </thead>
              <tbody>
                {breakdown.byModel.length ? (
                  breakdown.byModel.map((m) => (
                    <tr key={`${m.provider}/${m.model}`}>
                      <td className="mono">{m.model}</td>
                      <td>{formatCount(m.requests)}</td>
                      <td>{formatTokens(m.inputTokens)}</td>
                      <td>{formatTokens(m.outputTokens)}</td>
                      <td>{formatTokens(m.totalTokens)}</td>
                      <td>{formatLatency(m.avgLatencyMs)}</td>
                      <td>{m.estimatedCostUsd === null ? "price unknown" : formatCost(m.estimatedCostUsd)}</td>
                    </tr>
                  ))
                ) : (
                  <tr>
                    <td colSpan={7} className="muted">
                      No requests in this range.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </Card>
      </div>
    </Page>
  );
}
