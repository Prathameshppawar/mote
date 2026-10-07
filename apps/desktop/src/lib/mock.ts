/**
 * In-memory stand-in for the Rust backend, used when the UI runs in a plain
 * browser (design preview) and in tests. The UI labels this mode as sample
 * data; the real app always reads the local database through IPC.
 */
import type { ContextEvent } from "../bindings/ContextEvent";
import type { Diagnostics } from "../bindings/Diagnostics";
import type { EngineStatus } from "../bindings/EngineStatus";
import type { ExclusionRule } from "../bindings/ExclusionRule";
import type { Feature } from "../bindings/Feature";
import type { FeatureGroup } from "../bindings/FeatureGroup";
import type { ModelOption } from "../bindings/ModelOption";
import type { ModelPricing } from "../bindings/ModelPricing";
import type { PaletteContext } from "../bindings/PaletteContext";
import type { PeriodSummary } from "../bindings/PeriodSummary";
import type { RangeBreakdown } from "../bindings/RangeBreakdown";
import type { ProviderStatus } from "../bindings/ProviderStatus";
import type { SeriesPoint } from "../bindings/SeriesPoint";
import type { Settings } from "../bindings/Settings";
import type { UsageResponse } from "../bindings/UsageResponse";

type Listener = (payload: unknown) => void;
const listeners = new Map<string, Set<Listener>>();

export function mockEmit(event: string, payload: unknown): void {
  listeners.get(event)?.forEach((l) => l(payload));
}

/** Whether anything is subscribed to `event` (tests). */
export function mockHasListener(event: string): boolean {
  return (listeners.get(event)?.size ?? 0) > 0;
}

export function mockListen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  const set = listeners.get(event) ?? new Set<Listener>();
  set.add(handler as Listener);
  listeners.set(event, set);
  return Promise.resolve(() => set.delete(handler as Listener));
}

export function defaultSettings(): Settings {
  return {
    version: 1,
    general: { assistanceEnabled: true, launchAtLogin: false, onboardingCompleted: true, pausedUntil: null, theme: "system" },
    provider: {
      active: "groq",
      groq: {
        baseUrl: "https://api.groq.com/openai/v1",
        requestTimeoutMs: 20000,
        models: {
          completion: "qwen/qwen3.8-27b",
          classification: "qwen/qwen3.8-27b",
          writing: "qwen/qwen3.8-27b",
          reasoning: "openai/gpt-oss-120b",
          fallback: "openai/gpt-oss-20b",
        },
      },
    },
    completion: {
      enabled: true,
      debounceMs: 450,
      minChars: 12,
      maxWords: 12,
      minIntervalMs: 1200,
      inConversations: true,
      inPrompts: true,
      inNotes: true,
      inUnknown: false,
    },
    writing: { enabled: true, spelling: true, aiGrammar: true, ignoredWords: [] },
    prompts: { enhancementEnabled: true, showHint: true, defaultStyle: "improve" },
    context: { contextualSuggestions: true, clipboardTtlSecs: 180, aiClassification: true },
    privacy: { cloudAiEnabled: true, observeApplications: true, observeText: true, observeClipboard: true, contextRetention: "one_day" },
    usage: { analyticsEnabled: true, retentionDays: 180 },
    keyboard: { commandPalette: "CommandOrControl+Shift+Space", nextSuggestion: "Alt+BracketRight", previousSuggestion: "Alt+BracketLeft" },
  };
}

/** Deterministic pseudo-random numbers so previews and tests are stable. */
function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 0x1_0000_0000;
  };
}

const PRICES: Record<string, [number, number]> = {
  "qwen/qwen3.8-27b": [0.8, 4.0],
  "openai/gpt-oss-120b": [0.15, 0.6],
  "openai/gpt-oss-20b": [0.075, 0.3],
};

function isoDay(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

function summary(points: SeriesPoint[]): PeriodSummary {
  const sum = (f: (p: SeriesPoint) => number) => points.reduce((a, p) => a + f(p), 0);
  const requests = sum((p) => p.requests);
  const failed = sum((p) => p.failed);
  const latencyPoints = points.filter((p) => p.avgLatencyMs !== null);
  return {
    requests,
    successful: requests - failed,
    failed,
    rateLimited: Math.round(failed * 0.4),
    timeouts: Math.round(failed * 0.2),
    cancelled: Math.round(requests * 0.06),
    inputTokens: sum((p) => p.inputTokens),
    outputTokens: sum((p) => p.outputTokens),
    totalTokens: sum((p) => p.totalTokens),
    estimatedCostUsd: sum((p) => p.estimatedCostUsd),
    costComplete: true,
    avgLatencyMs: latencyPoints.length ? latencyPoints.reduce((a, p) => a + (p.avgLatencyMs ?? 0), 0) / latencyPoints.length : null,
    errorRate: requests ? failed / requests : 0,
  };
}

export function sampleUsage(now = new Date()): UsageResponse {
  const random = rng(7);
  const daily: SeriesPoint[] = [];
  for (let i = 29; i >= 0; i--) {
    const day = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i);
    const weekend = day.getDay() === 0 || day.getDay() === 6;
    const requests = Math.round((weekend ? 140 : 520) * (0.7 + random() * 0.6) * (1 + (29 - i) / 60));
    const input = requests * Math.round(110 + random() * 40);
    const output = requests * Math.round(14 + random() * 8);
    const failed = Math.round(requests * (0.004 + random() * 0.012));
    daily.push({
      label: isoDay(day),
      requests,
      failed,
      inputTokens: input,
      outputTokens: output,
      totalTokens: input + output,
      estimatedCostUsd: (input * 0.8 + output * 4) / 1e6,
      avgLatencyMs: 360 + random() * 140,
    });
  }
  const hourly: SeriesPoint[] = [];
  for (let h = 0; h <= now.getHours(); h++) {
    const active = h >= 9 && h <= 19;
    const requests = active ? Math.round(30 + random() * 50) : Math.round(random() * 4);
    const input = requests * 125;
    const output = requests * 18;
    hourly.push({
      label: `${String(h).padStart(2, "0")}:00`,
      requests,
      failed: requests > 40 ? 1 : 0,
      inputTokens: input,
      outputTokens: output,
      totalTokens: input + output,
      estimatedCostUsd: (input * 0.8 + output * 4) / 1e6,
      avgLatencyMs: requests ? 350 + random() * 120 : null,
    });
  }
  const today = summary(hourly);
  const month = summary(daily.filter((p) => p.label.slice(0, 7) === isoDay(now).slice(0, 7)));
  const week = summary(daily.slice(-((now.getDay() + 6) % 7) - 1));
  const last30 = summary(daily);
  const mix: [Feature, FeatureGroup, number][] = [
    ["inline_completion", "inline_completion", 0.61],
    ["writing_assistance", "writing_assistance", 0.14],
    ["prompt_enhancement", "prompt_enhancement", 0.11],
    ["intent_classification", "classification", 0.07],
    ["context_analysis", "context_analysis", 0.04],
    ["rewrite", "other", 0.02],
    ["translation", "other", 0.01],
  ];
  const breakdown = (s: PeriodSummary, latencyScale: number): RangeBreakdown => ({
    summary: s,
    byFeature: mix.map(([feature, group, share]) => ({
      feature,
      group,
      requests: Math.round(s.requests * share),
      totalTokens: Math.round(s.totalTokens * share),
      estimatedCostUsd: s.estimatedCostUsd * share,
      share,
      avgLatencyMs: (feature === "prompt_enhancement" ? 1240 : 390) * latencyScale,
    })),
    byModel: Object.entries(PRICES).map(([model, [inp, out]], i) => {
      const share = [0.86, 0.11, 0.03][i] ?? 0;
      const input = Math.round(s.inputTokens * share);
      const output = Math.round(s.outputTokens * share);
      return {
        provider: "groq",
        model,
        requests: Math.round(s.requests * share),
        inputTokens: input,
        outputTokens: output,
        totalTokens: input + output,
        estimatedCostUsd: (input * inp + output * out) / 1e6,
        avgLatencyMs: (i === 1 ? 1250 : 380) * latencyScale,
      };
    }),
    performance: {
      avgLatencyMs: 412 * latencyScale,
      medianLatencyMs: 371 * latencyScale,
      p95LatencyMs: 1180 * latencyScale,
      completionAvgLatencyMs: 384 * latencyScale,
      completionMedianLatencyMs: 352 * latencyScale,
      classificationAvgLatencyMs: 298 * latencyScale,
      classificationMedianLatencyMs: 274 * latencyScale,
      failedRequests: s.failed,
      rateLimitEvents: Math.round(s.requests * 0.002),
      timeouts: s.timeouts,
      cancelled: s.cancelled,
      errorRate: s.errorRate,
    },
    topFeature: s.requests ? { group: "inline_completion", share: 0.61 } : null,
  });
  return {
    analyticsEnabled: true,
    suggestionsToday: { shown: 182, accepted: 74, dismissed: 31 },
    suggestions30d: { shown: 3140, accepted: 1207, dismissed: 611 },
    dashboard: {
      generatedAt: now.toISOString(),
      today,
      week,
      month,
      last30Days: last30,
      breakdownToday: breakdown(today, 0.96),
      breakdown30d: breakdown(last30, 1),
      daily,
      hourly,
      providerLimits: {
        requestsLimit: 14400,
        requestsRemaining: 13212,
        requestsResetSecs: 2_861,
        tokensLimit: 18000,
        tokensRemaining: 16120,
        tokensResetSecs: 3.4,
        observedAt: now.toISOString(),
      },
      unpricedModels: [],
    },
  };
}

const state = {
  settings: defaultSettings(),
  hasKey: true,
  exclusions: [{ id: 1, kind: "window_title", pattern: "NetBanking", displayName: "NetBanking" }] as ExclusionRule[],
  pricing: Object.entries(PRICES).map(([model, [input, output]], i) => ({
    id: i + 1,
    provider: "groq",
    model,
    inputCostPerMillion: input,
    outputCostPerMillion: output,
    effectiveDate: "2026-10-07",
    source: "builtin",
  })) as ModelPricing[],
};

/** Commands invoked against the mock, in order (tests). */
export const mockCalls: { command: string; args: Record<string, unknown> }[] = [];

/** Makes palette_run take this long, so tests can act while it runs. */
let paletteRunDelayMs = 0;
export function setPaletteRunDelay(ms: number): void {
  paletteRunDelayMs = ms;
}

/** Resets mock state (tests). */
export function resetMock(): void {
  state.settings = defaultSettings();
  state.hasKey = true;
  mockCalls.length = 0;
  paletteRunDelayMs = 0;
}

function providerStatus(): ProviderStatus {
  return {
    provider: "groq",
    displayName: "Groq",
    hasApiKey: state.hasKey,
    keyHint: state.hasKey ? "…4Kcw" : null,
    baseUrl: state.settings.provider.groq.baseUrl,
    health: state.hasKey
      ? { ok: true, latencyMs: 212, modelsAvailable: 5, missingModels: [], message: null, checkedAt: new Date().toISOString() }
      : null,
    limits: sampleUsage().dashboard.providerLimits,
    unavailableModels: [],
  };
}

const engineStatus: EngineStatus = {
  state: "active",
  message: null,
  app: "Slack",
  intent: "conversation",
  subtype: "casual_message",
  intentConfidence: 0.86,
  language: "hinglish",
};

const models: ModelOption[] = [
  { id: "openai/gpt-oss-120b", ownedBy: "OpenAI", contextWindow: 131072, maxOutputTokens: 65536, inputCostPerMillion: 0.15, outputCostPerMillion: 0.6, recommendedFor: ["reasoning"] },
  { id: "openai/gpt-oss-20b", ownedBy: "OpenAI", contextWindow: 131072, maxOutputTokens: 65536, inputCostPerMillion: 0.075, outputCostPerMillion: 0.3, recommendedFor: [] },
  { id: "qwen/qwen3.8-27b", ownedBy: "Alibaba Cloud", contextWindow: 131072, maxOutputTokens: 16384, inputCostPerMillion: 0.8, outputCostPerMillion: 4, recommendedFor: ["completion", "classification", "writing"] },
];

function activity(): ContextEvent[] {
  const t = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();
  return [
    { timestamp: t(1), source: "mote", type: "suggestion_accepted", feature: "inline_completion" },
    { timestamp: t(1), source: "mote", type: "suggestion_shown", feature: "inline_completion" },
    { timestamp: t(2), source: "macos", type: "intent_classified", app: "Slack", kind: "conversation", confidence: 0.86 },
    { timestamp: t(2), source: "macos", type: "input_focused", app: "Slack", role: "text_area" },
    { timestamp: t(3), source: "macos", type: "application_changed", from: "Google Chrome", to: "Slack", category: "chat" },
    { timestamp: t(4), source: "macos", type: "clipboard_changed", source_app: "Google Chrome", kind: "email", char_count: 512 },
  ];
}

function diagnostics(): Diagnostics {
  return {
    version: "1.0.0",
    os: "macOS 26.3",
    arch: "aarch64",
    provider: "Groq",
    baseUrl: state.settings.provider.groq.baseUrl,
    hasApiKey: state.hasKey,
    models: state.settings.provider.groq.models,
    unavailableModels: [],
    providerHealth: providerStatus().health,
    providerLimits: providerStatus().limits,
    permissions: { accessibility: "granted", secureInputActive: false },
    clipboardObservation: state.settings.privacy.observeClipboard,
    textObservation: state.settings.privacy.observeText,
    cloudAiEnabled: state.settings.privacy.cloudAiEnabled,
    engine: { ...engineStatus, app: null },
    database: { schemaVersion: 1, usageEvents: 12034, contextEvents: 412, contextSessions: 88, exclusions: 1, pricingRows: 3, sizeBytes: 2_457_600 },
    databaseOk: true,
    databasePath: "~/Library/Application Support/io.github.prathameshppawar.mote/mote.db",
    logDir: "~/Library/Logs/io.github.prathameshppawar.mote",
    avgCompletionLatencyMs: 384,
    completionRequests24h: 611,
    lastRequest: { feature: "inline_completion", model: "qwen/qwen3.8-27b", status: "success", errorKind: null, latencyMs: 342, at: new Date().toISOString(), message: null },
  };
}

function paletteContext(): PaletteContext {
  return {
    appName: "ChatGPT",
    intent: "prompt",
    subtype: "coding",
    language: "english",
    languageName: "English",
    selection: null,
    field: { text: "fix this code it is giving error", chars: 32 },
    fieldTruncated: false,
    clipboard: { text: "TypeError: Cannot read properties of undefined (reading 'map')\n    at render (App.tsx:12:5)", chars: 96, sourceApp: "Terminal", kind: "stack_trace" },
    contextActions: ["debug_error", "analyze_issue", "create_prompt"],
    hasApiKey: true,
    cloudEnabled: true,
    canInsert: true,
  };
}

const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
  get_app_info: () => ({ version: "1.0.0", identifier: "io.github.prathameshppawar.mote", platform: "macos", arch: "aarch64", dataDir: "~/Library/Application Support/io.github.prathameshppawar.mote", logDir: "~/Library/Logs/io.github.prathameshppawar.mote" }),
  get_settings: () => structuredClone(state.settings),
  save_settings: (a) => {
    state.settings = structuredClone(a.settings as Settings);
    mockEmit("settings-changed", state.settings);
    return structuredClone(state.settings);
  },
  get_provider_status: () => providerStatus(),
  set_api_key: () => {
    state.hasKey = true;
    return providerStatus();
  },
  clear_api_key: () => {
    state.hasKey = false;
    return providerStatus();
  },
  test_connection: () => providerStatus().health ?? { ok: false, latencyMs: null, modelsAvailable: 0, missingModels: [], message: "Add your Groq API key in Settings → AI Providers.", checkedAt: new Date().toISOString() },
  list_models: () => models,
  get_usage_dashboard: () => sampleUsage(),
  list_pricing: () => state.pricing,
  save_pricing: (a) => {
    const p = a.pricing as ModelPricing;
    state.pricing = [...state.pricing.filter((x) => !(x.model === p.model && x.effectiveDate === p.effectiveDate && x.source === "user")), { ...p, id: Date.now(), source: "user" }];
    return state.pricing;
  },
  delete_pricing: (a) => {
    state.pricing = state.pricing.filter((p) => p.id !== a.id || p.source === "builtin");
    return state.pricing;
  },
  reset_pricing: () => {
    state.pricing = state.pricing.filter((p) => p.source === "builtin");
    return state.pricing;
  },
  clear_usage_history: () => 0,
  clear_context: () => undefined,
  reset_local_data: () => {
    state.settings = defaultSettings();
    state.settings.general.onboardingCompleted = false;
    state.hasKey = false;
    return structuredClone(state.settings);
  },
  list_exclusions: () => state.exclusions,
  add_exclusion: (a) => {
    state.exclusions = [...state.exclusions, { id: Date.now(), kind: a.kind as ExclusionRule["kind"], pattern: String(a.pattern), displayName: String(a.displayName || a.pattern) }];
    return state.exclusions;
  },
  remove_exclusion: (a) => {
    state.exclusions = state.exclusions.filter((e) => e.id !== a.id);
    return state.exclusions;
  },
  list_running_apps: () => [
    { id: "com.tinyspeck.slackmacgap", name: "Slack", pid: 101 },
    { id: "com.google.Chrome", name: "Google Chrome", pid: 102 },
    { id: "com.microsoft.VSCode", name: "Visual Studio Code", pid: 103 },
  ],
  get_always_excluded: () => [
    "Password managers (1Password, Bitwarden, KeePassXC, LastPass, Dashlane, Keychain Access, Passwords, and others)",
    "Password and other secure text fields in every application",
    "Any application while macOS Secure Input is active",
    "Mote's own windows",
  ],
  get_recent_activity: () => activity(),
  get_permission_status: () => ({ accessibility: "granted", secureInputActive: false }),
  request_accessibility_permission: () => "granted",
  open_accessibility_settings: () => undefined,
  get_diagnostics: () => diagnostics(),
  copy_diagnostics: () => "Mote Diagnostics (sample)",
  set_paused: (a) => {
    state.settings.general.pausedUntil = a.minutes ? new Date(Date.now() + Number(a.minutes) * 60_000).toISOString() : null;
    return structuredClone(state.settings);
  },
  complete_onboarding: () => {
    state.settings.general.onboardingCompleted = true;
    return structuredClone(state.settings);
  },
  get_engine_status: () => engineStatus,
  palette_context: () => paletteContext(),
  palette_run: async () => {
    if (paletteRunDelayMs) await new Promise((r) => setTimeout(r, paletteRunDelayMs));
    return {
      text: "Fix the issue in the following code. Identify the root cause, explain why the error occurs, and provide the smallest correct fix. Avoid unrelated changes.",
      source: "field",
      canReplace: true,
    };
  },
  palette_cancel: () => undefined,
  palette_apply: () => ({ applied: true, copied: false, message: null }),
  palette_close: () => undefined,
  palette_open_main: () => undefined,
  overlay_ready: () => undefined,
};

export async function mockInvoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const handler = handlers[command];
  if (!handler) {
    throw { code: "unknown_command", message: `Unknown command ${command}`, fields: [] };
  }
  mockCalls.push({ command, args });
  await new Promise((r) => setTimeout(r, 0));
  return (await handler(args)) as T;
}

/** Sample overlay payloads for the browser preview (overlay.html?demo=…). */
export function sampleOverlay(kind: string): import("../bindings/OverlayPayload").OverlayPayload | null {
  const anchor = { x: 0, y: 0, width: 1, height: 18 };
  const base = { index: 0, count: 1, anchor, coordinateSpace: "logical_points" as const, detail: null, acceptHint: "Tab" };
  switch (kind) {
    case "completion":
      return { seq: 1, view: { ...base, kind: "completion", text: " the Redis container was unavailable during startup.", count: 2 } };
    case "correction":
      return { seq: 1, view: { ...base, kind: "correction", text: "completed.", detail: "completd → completed" } };
    case "hint":
      return { seq: 1, view: { ...base, kind: "prompt_hint", text: "Enhance prompt", detail: "⌘⇧Space", acceptHint: null } };
    case "context":
      return {
        seq: 1,
        view: { ...base, kind: "context", text: "Use copied email from Google Chrome", detail: "⌘⇧Space · Create coding task · Analyze issue · Summarize", acceptHint: null },
      };
    case "notice":
      return { seq: 1, view: { ...base, kind: "notice", text: "Copied to clipboard", acceptHint: null } };
    default:
      return null;
  }
}
