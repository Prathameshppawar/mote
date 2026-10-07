import type { ContextAction } from "../bindings/ContextAction";
import type { EnhanceStyle } from "../bindings/EnhanceStyle";
import type { PaletteContext } from "../bindings/PaletteContext";
import type { TransformAction } from "../bindings/TransformAction";

export type Group = "Copied content" | "Prompt" | "Writing" | "Transform" | "Mote";

export type PaletteAction = {
  id: string;
  title: string;
  hint?: string;
  group: Group;
  keywords: string;
  /** The transformation to run. */
  action?: TransformAction;
  /** Needs a follow-up input before running. */
  needs?: "translate" | "custom";
  /** Opens a section of the main window instead. */
  section?: string;
  /** Runs on the copied content rather than the chosen source. */
  usesClipboard?: boolean;
};

const ENHANCE: { style: EnhanceStyle; title: string; hint: string }[] = [
  { style: "improve", title: "Improve prompt", hint: "Clearer and more specific, about the same length" },
  { style: "precise", title: "Make precise", hint: "Exact goal, inputs, constraints and output" },
  { style: "technical", title: "Make technical", hint: "Expert wording, technologies and edge cases" },
  { style: "structure", title: "Structure", hint: "Goal · Context · Requirements · Output" },
  { style: "debug", title: "Debug", hint: "Root cause, why it happens, smallest fix" },
  { style: "research", title: "Research", hint: "Balanced overview with sources" },
  { style: "explain", title: "Explain", hint: "A clear explanation with an example" },
  { style: "expand_context", title: "Expand context", hint: "Placeholders for missing details" },
];

const CONTEXT_TITLES: Record<ContextAction, string> = {
  create_coding_task: "Create coding task",
  analyze_issue: "Analyze issue",
  debug_error: "Debug this error",
  explain_code: "Explain code",
  summarize: "Summarize copied content",
  draft_response: "Draft a response",
  create_prompt: "Create prompt from copied content",
};

export const LANGUAGES = [
  "English",
  "Hindi (Devanagari)",
  "Hindi (Latin script)",
  "Marathi (Devanagari)",
  "Marathi (Latin script)",
  "Gujarati",
  "Bengali",
  "Tamil",
  "Telugu",
  "Spanish",
  "French",
  "German",
  "Portuguese",
  "Arabic",
  "Japanese",
  "Chinese (Simplified)",
];

/** All actions, ordered for the current context. */
export function buildActions(ctx: PaletteContext | null): PaletteAction[] {
  const copied: PaletteAction[] = [];
  if (ctx?.clipboard) {
    const actions: ContextAction[] = ctx.contextActions.length
      ? ctx.contextActions
      : ctx.intent === "conversation"
        ? ["draft_response", "summarize"]
        : ["create_prompt", "summarize", "analyze_issue"];
    for (const a of actions) {
      copied.push({
        id: `context:${a}`,
        title: CONTEXT_TITLES[a],
        hint: ctx.clipboard.sourceApp ? `From ${ctx.clipboard.sourceApp}` : "From the clipboard",
        group: "Copied content",
        keywords: "clipboard copied use",
        action: { kind: "use_context", action: a },
        usesClipboard: true,
      });
    }
  }
  const prompt: PaletteAction[] = [
    ...ENHANCE.map((e) => ({
      id: `enhance:${e.style}`,
      title: e.title,
      hint: e.hint,
      group: "Prompt" as const,
      keywords: "prompt enhance ai",
      action: { kind: "enhance_prompt", style: e.style } as TransformAction,
    })),
    { id: "enhance:custom", title: "Custom instruction…", hint: "Tell Mote how to change the prompt", group: "Prompt", keywords: "prompt custom", needs: "custom" },
    { id: "create_prompt", title: "Create prompt", hint: "Turn this text into a prompt for an AI assistant", group: "Prompt", keywords: "prompt create", action: { kind: "create_prompt" } },
  ];
  const writing: PaletteAction[] = [
    { id: "fix", title: "Fix spelling & grammar", hint: "Minimal corrections, same words", group: "Writing", keywords: "spelling grammar typo correct", action: { kind: "fix_spelling_grammar" } },
    { id: "improve", title: "Improve text", hint: "Clearer flow, same meaning", group: "Writing", keywords: "improve better", action: { kind: "improve_writing" } },
    { id: "rewrite", title: "Rewrite", hint: "Natural, fluent wording", group: "Writing", keywords: "rewrite rephrase", action: { kind: "rewrite" } },
    { id: "professional", title: "Make professional", group: "Writing", keywords: "tone formal professional", action: { kind: "professional" } },
    { id: "casual", title: "Make casual", group: "Writing", keywords: "tone casual friendly", action: { kind: "casual" } },
    { id: "clearer", title: "Make clearer", group: "Writing", keywords: "clarity simple", action: { kind: "clearer" } },
    { id: "concise", title: "Make concise", group: "Writing", keywords: "short concise", action: { kind: "concise" } },
  ];
  const transform: PaletteAction[] = [
    { id: "summarize", title: "Summarize", group: "Transform", keywords: "summary tldr", action: { kind: "summarize" } },
    { id: "explain", title: "Explain", group: "Transform", keywords: "explain meaning", action: { kind: "explain" } },
    { id: "translate", title: "Translate…", hint: "Choose a language", group: "Transform", keywords: "translate language hindi marathi", needs: "translate" },
    { id: "continue", title: "Continue writing", group: "Transform", keywords: "continue more", action: { kind: "continue_writing" } },
  ];
  const mote: PaletteAction[] = [
    { id: "open:usage", title: "Open Usage", group: "Mote", keywords: "usage tokens cost", section: "usage" },
    { id: "open:settings", title: "Open Settings", group: "Mote", keywords: "settings preferences", section: "general" },
  ];
  const ordered =
    ctx?.intent === "prompt"
      ? [...copied, ...prompt, ...writing, ...transform]
      : ctx?.intent === "conversation" || ctx?.intent === "note"
        ? [...copied, ...writing, ...transform, ...prompt]
        : [...copied, ...writing, ...prompt, ...transform];
  return [...ordered, ...mote];
}

export function filterActions(actions: PaletteAction[], query: string): PaletteAction[] {
  const q = query.trim().toLowerCase();
  if (!q) return actions;
  return actions.filter((a) => `${a.title} ${a.hint ?? ""} ${a.keywords} ${a.group}`.toLowerCase().includes(q));
}
