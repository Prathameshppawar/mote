import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import type { ApplyMode } from "../bindings/ApplyMode";
import type { PaletteContext } from "../bindings/PaletteContext";
import type { PaletteRunResult } from "../bindings/PaletteRunResult";
import type { PaletteSource } from "../bindings/PaletteSource";
import type { TransformAction } from "../bindings/TransformAction";
import { Icon } from "../components/Icon";
import { INTENT_LABELS, formatCount } from "../lib/format";
import { api, errorMessage, subscribe, toCommandError } from "../lib/ipc";
import { isMac } from "../lib/shortcuts";
import { buildActions, filterActions, LANGUAGES, type PaletteAction } from "./actions";

type Mode =
  | { kind: "list" }
  | { kind: "translate" }
  | { kind: "custom" }
  | { kind: "running"; title: string }
  | { kind: "result"; title: string; result: PaletteRunResult; text: string }
  | { kind: "error"; title: string; message: string; retry: () => void };

function defaultSource(ctx: PaletteContext | null): PaletteSource {
  if (ctx?.selection) return "selection";
  if (ctx?.field) return "field";
  return "clipboard";
}

const SOURCE_LABEL: Record<PaletteSource, string> = { selection: "Selection", field: "Text field", clipboard: "Clipboard" };
const mod = isMac ? "⌘" : "Ctrl+";

export function Palette() {
  const [ctx, setCtx] = useState<PaletteContext | null>(null);
  const [source, setSource] = useState<PaletteSource>("field");
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<Mode>({ kind: "list" });
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const resultBox = useRef<HTMLTextAreaElement>(null);

  const reset = useCallback(() => {
    setQuery("");
    setMode({ kind: "list" });
    setActive(0);
    api.paletteContext().then(
      (c) => {
        setCtx(c);
        setSource(defaultSource(c));
      },
      () => setCtx(null),
    );
    setTimeout(() => input.current?.focus(), 0);
  }, []);

  useEffect(() => {
    reset();
    const unlisten = subscribe("palette-open", reset);
    const onBlur = () => void api.paletteClose(false).catch(() => undefined);
    window.addEventListener("blur", onBlur);
    return () => {
      void unlisten.then((fn) => fn());
      window.removeEventListener("blur", onBlur);
    };
  }, [reset]);

  const actions = useMemo(() => buildActions(ctx), [ctx]);
  const languages = useMemo(() => LANGUAGES.filter((l) => l.toLowerCase().includes(query.trim().toLowerCase())), [query]);
  const visible = useMemo(() => filterActions(actions, query), [actions, query]);
  const sources = (["selection", "field", "clipboard"] as PaletteSource[]).filter((s) =>
    s === "selection" ? ctx?.selection : s === "field" ? ctx?.field : ctx?.clipboard,
  );
  const sourcePreview = source === "selection" ? ctx?.selection : source === "field" ? ctx?.field : ctx?.clipboard;

  const run = useCallback(
    async (action: TransformAction, title: string, usesClipboard = false) => {
      const runSource: PaletteSource = usesClipboard ? "field" : source;
      setMode({ kind: "running", title });
      try {
        const result = await api.paletteRun({ action, source: runSource });
        setMode({ kind: "result", title, result, text: result.text });
        setTimeout(() => resultBox.current?.focus(), 0);
      } catch (e) {
        const error = toCommandError(e);
        if (error.code === "cancelled") return setMode({ kind: "list" });
        setMode({ kind: "error", title, message: error.message, retry: () => void run(action, title, usesClipboard) });
      }
    },
    [source],
  );

  const choose = (item: PaletteAction | undefined) => {
    if (!item) return;
    if (item.section) return void api.paletteOpenMain(item.section);
    if (item.needs === "translate") {
      setQuery("");
      setActive(0);
      return setMode({ kind: "translate" });
    }
    if (item.needs === "custom") {
      setQuery("");
      return setMode({ kind: "custom" });
    }
    if (item.action) void run(item.action, item.title, item.usesClipboard);
  };

  const apply = async (applyMode: ApplyMode) => {
    if (mode.kind !== "result") return;
    try {
      await api.paletteApply({ text: mode.text, mode: applyMode });
    } catch (e) {
      setMode({ kind: "error", title: mode.title, message: errorMessage(e), retry: () => setMode(mode) });
    }
  };

  const close = () => void api.paletteClose(true).catch(() => undefined);

  const onKeyDown = (e: KeyboardEvent) => {
    const list = mode.kind === "translate" ? languages : visible;
    if (e.key === "Escape") {
      e.preventDefault();
      if (mode.kind === "running") {
        void api.paletteCancel();
        return setMode({ kind: "list" });
      }
      if (mode.kind === "list") {
        if (query) return setQuery("");
        return close();
      }
      setQuery("");
      return setMode({ kind: "list" });
    }
    if (mode.kind === "result") {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        return void apply("insert");
      }
      if (e.key === "Enter" && !e.shiftKey && document.activeElement !== resultBox.current) {
        e.preventDefault();
        return void apply(mode.result.canReplace ? "replace" : "insert");
      }
      return;
    }
    if (mode.kind !== "list" && mode.kind !== "translate") return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(list.length - 1, a + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === "Tab" && mode.kind === "list" && sources.length > 1) {
      e.preventDefault();
      const i = sources.indexOf(source);
      setSource(sources[(i + (e.shiftKey ? sources.length - 1 : 1)) % sources.length] ?? source);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (mode.kind === "translate") {
        const target = languages[active];
        if (target) void run({ kind: "translate", target }, `Translate to ${target}`);
      } else {
        choose(visible[active]);
      }
    }
  };

  useEffect(() => setActive(0), [query, mode.kind]);

  const header = (
    <div className="palette-head">
      <Icon name="sparkle" size={18} />
      {mode.kind === "list" || mode.kind === "translate" || mode.kind === "custom" ? (
        <input
          ref={input}
          className="palette-input"
          placeholder={
            mode.kind === "translate"
              ? "Translate to…"
              : mode.kind === "custom"
                ? "How should Mote change the prompt? Press Enter"
                : "What should Mote do?"
          }
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (mode.kind === "custom" && e.key === "Enter" && query.trim()) {
              e.preventDefault();
              void run({ kind: "custom_enhance", instruction: query.trim() }, "Custom instruction");
              return;
            }
            onKeyDown(e);
          }}
          aria-label="Search actions"
          aria-controls="palette-list"
          autoFocus
        />
      ) : (
        <span className="palette-title">{mode.title}</span>
      )}
      {ctx?.appName ? <span className="badge">{ctx.appName}</span> : null}
    </div>
  );

  return (
    <div className="palette" onKeyDown={mode.kind === "result" || mode.kind === "running" || mode.kind === "error" ? onKeyDown : undefined}>
      {header}

      {mode.kind === "list" ? (
        <>
          <div className="palette-context">
            <div className="source-tabs" role="tablist" aria-label="Text to use">
              {sources.length ? (
                sources.map((s) => (
                  <button key={s} type="button" role="tab" aria-selected={s === source} onClick={() => setSource(s)}>
                    {SOURCE_LABEL[s]}
                  </button>
                ))
              ) : (
                <span className="muted">Select text or type in a field first, or copy something.</span>
              )}
            </div>
            {ctx?.intent ? <span className="muted">{INTENT_LABELS[ctx.intent]}{ctx.languageName && ctx.languageName !== "English" ? ` · ${ctx.languageName}` : ""}</span> : null}
          </div>
          {sourcePreview ? (
            <p className="palette-preview">
              {sourcePreview.text}
              {sourcePreview.chars > sourcePreview.text.length ? "…" : ""}
              <span className="muted"> · {formatCount(sourcePreview.chars)} characters</span>
            </p>
          ) : null}
          <ul className="palette-list" id="palette-list" role="listbox" aria-label="Actions">
            {visible.map((item, i) => {
              const showGroup = i === 0 || visible[i - 1]?.group !== item.group;
              return (
                <li key={item.id} role="presentation">
                  {showGroup ? <div className="palette-group">{item.group}</div> : null}
                  <button
                    type="button"
                    role="option"
                    aria-selected={i === active}
                    className="palette-item"
                    onMouseEnter={() => setActive(i)}
                    onClick={() => choose(item)}
                  >
                    <span>{item.title}</span>
                    {item.hint ? <span className="muted">{item.hint}</span> : null}
                  </button>
                </li>
              );
            })}
            {!visible.length ? <li className="palette-empty muted">No matching actions</li> : null}
          </ul>
        </>
      ) : null}

      {mode.kind === "translate" ? (
        <ul className="palette-list" role="listbox" aria-label="Languages">
          {languages.map((l, i) => (
            <li key={l} role="presentation">
              <button
                type="button"
                role="option"
                aria-selected={i === active}
                className="palette-item"
                onMouseEnter={() => setActive(i)}
                onClick={() => void run({ kind: "translate", target: l }, `Translate to ${l}`)}
              >
                {l}
              </button>
            </li>
          ))}
        </ul>
      ) : null}

      {mode.kind === "custom" ? (
        <p className="palette-hint muted">Example: “ask for TypeScript, keep it under 100 words”. Mote rewrites the prompt; it never answers it.</p>
      ) : null}

      {mode.kind === "running" ? (
        <div className="palette-status">
          <span className="spinner" />
          Asking Groq… <span className="muted">Esc to cancel</span>
        </div>
      ) : null}

      {mode.kind === "error" ? (
        <div className="palette-status" role="alert">
          <Icon name="alert" />
          <span>{mode.message}</span>
          <button type="button" className="btn small" onClick={mode.retry}>
            Retry
          </button>
        </div>
      ) : null}

      {mode.kind === "result" ? (
        <div className="palette-result">
          <textarea
            ref={resultBox}
            className="textarea"
            value={mode.text}
            aria-label="Result (editable)"
            onChange={(e) => setMode({ ...mode, text: e.target.value })}
            rows={8}
          />
          <div className="palette-actions">
            {mode.result.canReplace ? (
              <button type="button" className="btn primary" onClick={() => apply("replace")}>
                Replace <kbd className="kbd">↩</kbd>
              </button>
            ) : null}
            <button type="button" className={`btn ${mode.result.canReplace ? "" : "primary"}`} onClick={() => apply("insert")}>
              Insert <kbd className="kbd">{mod}↩</kbd>
            </button>
            <button type="button" className="btn" onClick={() => apply("copy")}>
              Copy
            </button>
            <span className="muted" style={{ marginLeft: "auto", fontSize: 12 }}>
              Esc to go back
            </span>
          </div>
        </div>
      ) : null}

      <footer className="palette-foot muted">
        {ctx && !ctx.hasApiKey ? "Add a Groq API key in Settings to run actions." : ctx && !ctx.cloudEnabled ? "Cloud AI is off in Privacy settings." : "Text leaves your computer only when you run an action."}
        <span>
          <kbd className="kbd">↑↓</kbd> choose <kbd className="kbd">↩</kbd> run {sources.length > 1 ? <><kbd className="kbd">Tab</kbd> source </> : null}
          <kbd className="kbd">Esc</kbd> close
        </span>
      </footer>
    </div>
  );
}
