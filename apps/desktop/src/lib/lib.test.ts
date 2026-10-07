import { describe, expect, it } from "vitest";

import { columnPath, labelIndices, niceTicks } from "../charts/scale";
import { describeEvent } from "./activity";
import { formatCost, formatCount, formatDayLabel, formatLatency, formatPercent, formatRelative, formatSeconds, formatTokens, parsePrice, formatCostEstimate } from "./format";
import { toCommandError } from "./ipc";
import { acceleratorFromEvent, shortcutLabel } from "./shortcuts";

describe("format", () => {
  it("formats token counts compactly like the spec examples", () => {
    expect(formatTokens(980)).toBe("980");
    expect(formatTokens(42_300)).toBe("42.3K");
    expect(formatTokens(1_840_000)).toBe("1.84M");
    expect(formatTokens(1_500)).toBe("1.5K");
  });

  it("formats counts with separators", () => {
    expect(formatCount(1284)).toBe("1,284");
    expect(formatCount(28412)).toBe("28,412");
  });

  it("parses prices typed with a dot or a comma", () => {
    expect(parsePrice("0.15")).toBe(0.15);
    expect(parsePrice(" 1,5 ")).toBe(1.5);
    expect(parsePrice("")).toBeNull();
    expect(parsePrice("abc")).toBeNull();
    expect(parsePrice("-1")).toBeNull();
  });

  it("marks incomplete cost estimates", () => {
    expect(formatCostEstimate(0.42, true)).toBe("$0.42");
    expect(formatCostEstimate(0.42, false)).toBe("$0.42+");
  });

  it("formats estimated cost", () => {
    expect(formatCost(0)).toBe("$0.00");
    expect(formatCost(0.12)).toBe("$0.12");
    expect(formatCost(4.834)).toBe("$4.83");
    expect(formatCost(0.0042)).toBe("$0.0042");
  });

  it("formats latency, percentages and durations", () => {
    expect(formatLatency(420)).toBe("420 ms");
    expect(formatLatency(1250)).toBe("1.3 s");
    expect(formatLatency(null)).toBe("—");
    expect(formatPercent(0.005)).toBe("0.5%");
    expect(formatPercent(0.61)).toBe("61%");
    expect(formatPercent(0)).toBe("0%");
    expect(formatSeconds(86.4)).toBe("1 min 26 s");
    expect(formatSeconds(3.4)).toBe("3 s");
    expect(formatDayLabel("2026-10-07")).toBe("Oct 7");
  });

  it("formats relative times", () => {
    const now = new Date("2026-10-07T12:00:00Z");
    expect(formatRelative("2026-10-07T11:59:50Z", now)).toBe("just now");
    expect(formatRelative("2026-10-07T11:55:00Z", now)).toBe("5 min ago");
    expect(formatRelative("2026-10-07T09:00:00Z", now)).toBe("3 h ago");
  });
});

describe("shortcuts", () => {
  it("labels accelerators per platform", () => {
    expect(shortcutLabel("CommandOrControl+Shift+Space", true)).toBe("⌘⇧Space");
    expect(shortcutLabel("CommandOrControl+Shift+Space", false)).toBe("Ctrl+Shift+Space");
    expect(shortcutLabel("Alt+BracketRight", true)).toBe("⌥]");
    expect(shortcutLabel("Alt+BracketLeft", false)).toBe("Alt+[");
  });

  it("converts key events into accelerators", () => {
    const base = { metaKey: false, ctrlKey: false, altKey: false, shiftKey: false };
    expect(acceleratorFromEvent({ ...base, code: "Space", metaKey: true, shiftKey: true }, true)).toBe("CommandOrControl+Shift+Space");
    expect(acceleratorFromEvent({ ...base, code: "KeyM", ctrlKey: true, altKey: true }, false)).toBe("CommandOrControl+Alt+M");
    expect(acceleratorFromEvent({ ...base, code: "BracketRight", altKey: true }, true)).toBe("Alt+BracketRight");
    expect(acceleratorFromEvent({ ...base, code: "KeyA" }, true)).toBeNull();
    expect(acceleratorFromEvent({ ...base, code: "ShiftLeft", shiftKey: true }, true)).toBeNull();
    expect(acceleratorFromEvent({ ...base, code: "KeyK", shiftKey: true }, true)).toBeNull();
  });
});

describe("chart scales", () => {
  it("uses whole-number ticks for counts", () => {
    expect(niceTicks(1, 4, true)).toEqual([0, 1]);
    expect(niceTicks(2, 4, true)).toEqual([0, 1, 2]);
    expect(niceTicks(7, 4, true)).toEqual([0, 2, 4, 6, 8]);
    expect(niceTicks(87, 4, true)).toEqual([0, 20, 40, 60, 80, 100]);
  });

  it("produces clean ticks from zero", () => {
    expect(niceTicks(87)).toEqual([0, 20, 40, 60, 80, 100]);
    expect(niceTicks(1_300)).toEqual([0, 500, 1000, 1500]);
    expect(niceTicks(0)).toEqual([0, 1]);
    expect(niceTicks(0.18)).toEqual([0, 0.05, 0.1, 0.15, 0.2]);
  });

  it("spreads axis labels and always includes both ends", () => {
    expect(labelIndices(30)).toEqual([0, 7, 15, 22, 29]);
    expect(labelIndices(3)).toEqual([0, 1, 2]);
    expect(labelIndices(0)).toEqual([]);
  });

  it("rounds only the data end of columns", () => {
    const d = columnPath(10, 20, 12, 100);
    expect(d.startsWith("M10,100V24")).toBe(true);
    expect(d.endsWith("V100Z")).toBe(true);
    expect(columnPath(10, 100, 12, 100)).toBe("");
  });
});

describe("activity descriptions", () => {
  it("never include content", () => {
    const text = describeEvent({
      timestamp: "2026-10-07T10:00:00Z",
      source: "macos",
      type: "clipboard_changed",
      source_app: "Mail",
      kind: "email",
      char_count: 512,
    });
    expect(text).toContain("512 characters");
    expect(text).toContain("content not stored");
  });
});

describe("ipc errors", () => {
  it("normalizes backend errors", () => {
    expect(toCommandError({ code: "validation", message: "Bad", fields: [] }).code).toBe("validation");
    expect(toCommandError("boom")).toEqual({ code: "unknown", message: "boom", fields: [] });
    expect(toCommandError(undefined).message).toBe("Something went wrong.");
  });
});
