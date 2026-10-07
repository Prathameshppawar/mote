/** Nice axis ticks: clean numbers from zero to just above `max`. For counts
 * (`integer`), steps are whole numbers so small values don't produce 0.5 ticks. */
export function niceTicks(max: number, target = 4, integer = false): number[] {
  if (!Number.isFinite(max) || max <= 0) return [0, 1];
  const rough = max / target;
  const magnitude = 10 ** Math.floor(Math.log10(rough));
  const residual = rough / magnitude;
  // Pick the nice step (1, 2, 5, 10 × magnitude) nearest the rough step.
  const nice = (residual < 1.5 ? 1 : residual < 3 ? 2 : residual < 7 ? 5 : 10) * magnitude;
  const step = integer ? Math.max(1, Math.round(nice)) : nice;
  const top = Math.ceil(max / step) * step;
  const ticks: number[] = [];
  for (let v = 0; v <= top + step / 2; v += step) ticks.push(Number(v.toPrecision(12)));
  return ticks;
}

/** Evenly spaced indices for sparse x-axis labels (always first and last). */
export function labelIndices(count: number, maxLabels = 5): number[] {
  if (count <= 0) return [];
  if (count <= maxLabels) return Array.from({ length: count }, (_, i) => i);
  const out = new Set<number>();
  for (let i = 0; i < maxLabels; i++) out.add(Math.round((i * (count - 1)) / (maxLabels - 1)));
  return [...out].sort((a, b) => a - b);
}

/** SVG path for a column with a 4px rounded data end and a square baseline. */
export function columnPath(x: number, y: number, width: number, baseline: number, radius = 4): string {
  const h = baseline - y;
  if (h <= 0.5) return "";
  const r = Math.min(radius, width / 2, h);
  return `M${x},${baseline}V${y + r}Q${x},${y} ${x + r},${y}H${x + width - r}Q${x + width},${y} ${x + width},${y + r}V${baseline}Z`;
}
