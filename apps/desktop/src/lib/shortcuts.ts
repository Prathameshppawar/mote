/** Shortcut helpers: display labels and keyboard-event → accelerator conversion. */

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

const KEY_LABELS: Record<string, string> = {
  bracketright: "]",
  bracketleft: "[",
  space: "Space",
  escape: "Esc",
  enter: "↩",
  backspace: "⌫",
  comma: ",",
  period: ".",
  slash: "/",
  backslash: "\\",
  semicolon: ";",
  quote: "'",
  minus: "-",
  equal: "=",
  backquote: "`",
};

/** "CommandOrControl+Shift+Space" → "⌘⇧Space" (macOS) or "Ctrl+Shift+Space". */
export function shortcutLabel(accelerator: string, mac = isMac): string {
  return accelerator
    .split("+")
    .map((part) => {
      const p = part.toLowerCase();
      if (p === "commandorcontrol" || p === "cmdorctrl") return mac ? "⌘" : "Ctrl+";
      if (p === "command" || p === "cmd" || p === "super" || p === "meta") return mac ? "⌘" : "Win+";
      if (p === "control" || p === "ctrl") return mac ? "⌃" : "Ctrl+";
      if (p === "alt" || p === "option") return mac ? "⌥" : "Alt+";
      if (p === "shift") return mac ? "⇧" : "Shift+";
      if (KEY_LABELS[p]) return KEY_LABELS[p];
      if (p.startsWith("key") && p.length === 4) return p.slice(3).toUpperCase();
      if (p.startsWith("digit") && p.length === 6) return p.slice(5);
      return part.length === 1 ? part.toUpperCase() : part;
    })
    .join("");
}

const MODIFIER_CODES = new Set(["ShiftLeft", "ShiftRight", "ControlLeft", "ControlRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight"]);

/** Converts a keydown into an accelerator, or null if incomplete/unsupported. */
export function acceleratorFromEvent(
  e: Pick<KeyboardEvent, "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">,
  mac = isMac,
): string | null {
  if (MODIFIER_CODES.has(e.code)) return null;
  const parts: string[] = [];
  if (mac ? e.metaKey : e.ctrlKey) parts.push("CommandOrControl");
  if (mac && e.ctrlKey) parts.push("Control");
  if (!mac && e.metaKey) parts.push("Super");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (!parts.length) return null;
  let key: string;
  if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
  else if (/^Digit[0-9]$/.test(e.code)) key = e.code.slice(5);
  else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.code)) key = e.code;
  else if (
    ["Space", "BracketLeft", "BracketRight", "Comma", "Period", "Slash", "Backslash", "Semicolon", "Quote", "Minus", "Equal", "Backquote", "Enter", "Backspace"].includes(e.code)
  )
    key = e.code;
  else return null;
  return [...parts, key].join("+");
}
