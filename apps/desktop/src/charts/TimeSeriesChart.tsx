import { useMemo, useState, type KeyboardEvent, type PointerEvent } from "react";

import { columnPath, labelIndices, niceTicks } from "./scale";
import { useWidth } from "./useWidth";

export type SeriesDatum = { label: string; value: number | null };

type Props = {
  points: SeriesDatum[];
  variant: "area" | "line" | "columns";
  format: (value: number) => string;
  formatLabel?: (label: string) => string;
  /** Accessible description, e.g. "Tokens per day, last 30 days". */
  ariaLabel: string;
  height?: number;
  /** Values are counts: use whole-number axis ticks. */
  integer?: boolean;
};

const M = { top: 12, right: 14, bottom: 26, left: 52 };

/**
 * A single-series time chart (one series → no legend; the card title names it).
 * Lines get a crosshair that snaps to the nearest point; columns are their own
 * hit targets. Both respond to keyboard focus and arrow keys.
 */
export function TimeSeriesChart({ points, variant, format, formatLabel = (l) => l, ariaLabel, height = 190, integer = false }: Props) {
  const [containerRef, width] = useWidth<HTMLDivElement>();
  const [active, setActive] = useState<number | null>(null);
  const n = points.length;
  const values = points.map((p) => p.value ?? 0);
  const ticks = useMemo(() => niceTicks(Math.max(0, ...values), 4, integer), [values, integer]);
  const top = ticks[ticks.length - 1] || 1;
  const plotW = Math.max(40, width - M.left - M.right);
  const plotH = height - M.top - M.bottom;
  const baseline = M.top + plotH;
  const y = (v: number) => M.top + plotH - (v / top) * plotH;
  const band = n > 0 ? plotW / n : plotW;
  const x = (i: number) => (variant === "columns" ? M.left + band * i + band / 2 : M.left + (n <= 1 ? plotW / 2 : (i * plotW) / (n - 1)));
  const barWidth = Math.max(2, Math.min(24, band - 2));

  let line = "";
  let started = false;
  points.forEach((p, i) => {
    if (p.value === null) {
      started = false;
      return;
    }
    line += `${started ? "L" : "M"}${x(i).toFixed(1)},${y(p.value).toFixed(1)}`;
    started = true;
  });
  const valid = points.map((p, i) => (p.value === null ? -1 : i)).filter((i) => i >= 0);
  const firstIndex = valid[0];
  const lastIndex = valid[valid.length - 1];
  const area =
    variant === "area" && firstIndex !== undefined && lastIndex !== undefined
      ? `${line}L${x(lastIndex).toFixed(1)},${baseline}L${x(firstIndex).toFixed(1)},${baseline}Z`
      : "";

  const nearest = (clientX: number, rect: DOMRect) => {
    const px = clientX - rect.left - M.left;
    const i = variant === "columns" ? Math.floor(px / band) : Math.round((px / plotW) * (n - 1));
    return Math.min(n - 1, Math.max(0, i));
  };

  const onPointerMove = (e: PointerEvent<SVGRectElement>) => {
    const svg = e.currentTarget.ownerSVGElement;
    if (svg) setActive(nearest(e.clientX, svg.getBoundingClientRect()));
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowRight") setActive((a) => Math.min(n - 1, (a ?? -1) + 1));
    else if (e.key === "ArrowLeft") setActive((a) => Math.max(0, (a ?? n) - 1));
    else if (e.key === "Home") setActive(0);
    else if (e.key === "End") setActive(n - 1);
    else if (e.key === "Escape") setActive(null);
    else return;
    e.preventDefault();
  };

  const activePoint = active !== null ? points[active] : undefined;
  const tooltipLeft = active !== null ? Math.min(Math.max(x(active), 70), width - 70) : 0;

  return (
    <div
      ref={containerRef}
      className="chart"
      tabIndex={0}
      role="group"
      aria-roledescription="chart"
      aria-label={`${ariaLabel}. Use the arrow keys to read values; the table view lists them all.`}
      onKeyDown={onKeyDown}
      onFocus={() => setActive((a) => a ?? lastIndex ?? null)}
      onBlur={() => setActive(null)}
    >
      <svg width={width} height={height} className="chart-svg" aria-hidden="true">
        {ticks.map((t) => (
          <g key={t}>
            <line
              x1={M.left}
              x2={M.left + plotW}
              y1={y(t)}
              y2={y(t)}
              className={t === 0 ? "chart-baseline" : "chart-grid"}
            />
            <text x={M.left - 8} y={y(t)} className="chart-tick" textAnchor="end" dominantBaseline="middle">
              {format(t)}
            </text>
          </g>
        ))}
        {labelIndices(n).map((i) => (
          <text key={i} x={x(i)} y={height - 6} className="chart-tick" textAnchor={i === 0 && variant !== "columns" ? "start" : i === n - 1 && variant !== "columns" ? "end" : "middle"}>
            {formatLabel(points[i]?.label ?? "")}
          </text>
        ))}

        {variant === "columns"
          ? points.map((p, i) => {
              const d = columnPath(x(i) - barWidth / 2, y(p.value ?? 0), barWidth, baseline);
              return d ? <path key={p.label} d={d} className="chart-column" data-active={active === i || undefined} /> : null;
            })
          : null}
        {area ? <path d={area} className="chart-area" /> : null}
        {variant !== "columns" && line ? <path d={line} className="chart-line" /> : null}

        {variant !== "columns" && lastIndex !== undefined && points[lastIndex]?.value !== null ? (
          <circle cx={x(lastIndex)} cy={y(points[lastIndex]?.value ?? 0)} r={4} className="chart-dot" />
        ) : null}

        {activePoint && active !== null && variant !== "columns" ? (
          <g>
            <line x1={x(active)} x2={x(active)} y1={M.top} y2={baseline} className="chart-crosshair" />
            {activePoint.value !== null ? <circle cx={x(active)} cy={y(activePoint.value)} r={4} className="chart-dot" /> : null}
          </g>
        ) : null}

        <rect
          x={M.left}
          y={M.top}
          width={plotW}
          height={plotH}
          fill="transparent"
          onPointerMove={onPointerMove}
          onPointerLeave={() => setActive(null)}
        />
      </svg>
      {activePoint ? (
        <div className="chart-tooltip" style={{ left: tooltipLeft }} aria-hidden="true">
          <strong>{activePoint.value === null ? "No data" : format(activePoint.value)}</strong>
          <span>{formatLabel(activePoint.label)}</span>
        </div>
      ) : null}
      {/* Always mounted, so screen readers announce each value as it changes. */}
      <div className="sr-only" role="status" aria-live="polite">
        {activePoint
          ? `${formatLabel(activePoint.label)}: ${activePoint.value === null ? "no data" : format(activePoint.value)}`
          : ""}
      </div>
    </div>
  );
}
