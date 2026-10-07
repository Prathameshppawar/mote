/** Number and time formatting shared by the UI. */

const integer = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

/** 1,284 */
export function formatCount(value: number): string {
  return integer.format(Math.round(value));
}

/** Compact token counts: 980, 42.3K, 1.84M. */
export function formatTokens(value: number): string {
  const abs = Math.abs(value);
  if (abs < 1_000) return integer.format(value);
  if (abs < 1_000_000) return `${trim(value / 1_000, abs < 10_000 ? 2 : 1)}K`;
  if (abs < 1_000_000_000) return `${trim(value / 1_000_000, abs < 10_000_000 ? 2 : 1)}M`;
  return `${trim(value / 1_000_000_000, 2)}B`;
}

function trim(value: number, digits: number): string {
  return value.toFixed(digits).replace(/\.?0+$/, "");
}

/** Estimated cost: $4.83, $0.12, $0.0042, $0.00. */
export function formatCost(usd: number): string {
  if (usd === 0) return "$0.00";
  if (Math.abs(usd) >= 1) return `$${usd.toFixed(2)}`;
  if (Math.abs(usd) >= 0.01) return `$${usd.toFixed(2)}`;
  return `$${usd.toPrecision(2)}`;
}

/** 420 ms, 1.2 s */
export function formatLatency(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return "—";
  if (ms < 1_000) return `${Math.round(ms)} ms`;
  return `${(ms / 1_000).toFixed(ms < 10_000 ? 1 : 0)} s`;
}

/** 0.005 → 0.5%, 0.61 → 61% */
export function formatPercent(ratio: number): string {
  if (!Number.isFinite(ratio)) return "—";
  const pct = ratio * 100;
  if (pct === 0) return "0%";
  if (pct < 1) return `${pct.toFixed(1)}%`;
  return `${Math.round(pct)}%`;
}

/** "just now", "5 min ago", "3 h ago", or a date. */
export function formatRelative(iso: string, now: Date = new Date()): string {
  const then = new Date(iso);
  const seconds = Math.round((now.getTime() - then.getTime()) / 1000);
  if (!Number.isFinite(seconds)) return "";
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return then.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** Time of day: 14:05 */
export function formatClock(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** "2026-10-07" → "Oct 7" */
export function formatDayLabel(day: string): string {
  const [y, m, d] = day.split("-").map(Number);
  if (!y || !m || !d) return day;
  return new Date(y, m - 1, d).toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

/** Seconds → "1 min 26 s" / "7 s". */
export function formatSeconds(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return "—";
  if (seconds < 60) return `${Math.max(1, Math.round(seconds))} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ${Math.round(seconds % 60)} s`;
  const hours = Math.floor(minutes / 60);
  return `${hours} h ${minutes % 60} min`;
}

/** Human labels for stable identifiers. */
export const FEATURE_LABELS: Record<string, string> = {
  inline_completion: "Inline Completion",
  prompt_enhancement: "Prompt Enhancement",
  writing_assistance: "Writing Assistance",
  intent_classification: "Classification",
  context_analysis: "Context Analysis",
  translation: "Translation",
  rewrite: "Rewrite",
  command_interface: "Command Interface",
};

export const FEATURE_GROUP_LABELS: Record<string, string> = {
  inline_completion: "Inline Completion",
  prompt_enhancement: "Prompt Enhancement",
  writing_assistance: "Writing Assistance",
  classification: "Classification",
  context_analysis: "Context Analysis",
  other: "Other",
};

export const INTENT_LABELS: Record<string, string> = {
  conversation: "Conversation",
  prompt: "AI prompt",
  code: "Code",
  command: "Command",
  note: "Note",
  search: "Search",
  form: "Form",
  unknown: "Unknown",
};

export const SUBTYPE_LABELS: Record<string, string> = {
  email: "Email",
  chat: "Chat",
  professional_message: "Professional message",
  casual_message: "Casual message",
  coding: "Coding",
  research: "Research",
  reasoning: "Reasoning",
  general: "General",
};
