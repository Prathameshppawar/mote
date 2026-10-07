import { useState } from "react";

import { Card, Kbd, Note, Page, Row, Slider, ToggleRow } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { shortcutLabel } from "../../lib/shortcuts";
import { useSettings } from "../settingsContext";

export function CompletionSection() {
  const { settings, update } = useSettings();
  if (!settings) return null;
  const c = settings.completion;
  return (
    <Page title="Completion" subtitle="As you type, Mote suggests a short continuation. Press Tab to accept it; keep typing to ignore it.">
      <Card>
        <ToggleRow title="Inline completion" checked={c.enabled} onChange={(v) => update((s) => void (s.completion.enabled = v))} />
        <Row
          title="Keys"
          help={
            <>
              Mote only claims these keys while a suggestion is visible. A Tab with no suggestion passes through.
            </>
          }
        >
          <span className="muted" style={{ display: "flex", gap: 6, alignItems: "center", flexWrap: "wrap" }}>
            <Kbd>Tab</Kbd> accept <Kbd>Esc</Kbd> dismiss <Kbd>{shortcutLabel(settings.keyboard.nextSuggestion)}</Kbd> next{" "}
            <Kbd>{shortcutLabel(settings.keyboard.previousSuggestion)}</Kbd> previous
          </span>
        </Row>
      </Card>

      <div className="section-label">Where</div>
      <Card>
        <ToggleRow
          title="Messages and email"
          help="Chat apps, email and other conversations."
          checked={c.inConversations}
          onChange={(v) => update((s) => void (s.completion.inConversations = v))}
          disabled={!c.enabled}
        />
        <ToggleRow
          title="AI prompts"
          help="ChatGPT, Claude, Copilot chat and other prompt boxes. Mote continues the prompt; it never answers it."
          checked={c.inPrompts}
          onChange={(v) => update((s) => void (s.completion.inPrompts = v))}
          disabled={!c.enabled}
        />
        <ToggleRow
          title="Notes and documents"
          checked={c.inNotes}
          onChange={(v) => update((s) => void (s.completion.inNotes = v))}
          disabled={!c.enabled}
        />
        <ToggleRow
          title="Unclassified text fields"
          help="Fields Mote can't place. Off by default to save tokens."
          checked={c.inUnknown}
          onChange={(v) => update((s) => void (s.completion.inUnknown = v))}
          disabled={!c.enabled}
        />
        <Row title="Code, terminals, search and forms" help="Never completed: your editor and shell have their own tools, and short fields don't benefit.">
          <span className="badge">Always off</span>
        </Row>
      </Card>

      <div className="section-label">Timing and length</div>
      <Card>
        <Row title="Wait after typing" help="Pause before asking for a suggestion. Longer waits use fewer tokens.">
          <Slider label="Wait after typing" value={c.debounceMs} min={200} max={1500} step={50} format={(v) => `${v} ms`} onCommit={(v) => update((s) => void (s.completion.debounceMs = v))} />
        </Row>
        <Row title="Minimum gap between requests" help="Caps how often Mote calls Groq while you type continuously.">
          <Slider label="Minimum gap" value={c.minIntervalMs} min={0} max={5000} step={100} format={(v) => (v ? `${(v / 1000).toFixed(1)} s` : "none")} onCommit={(v) => update((s) => void (s.completion.minIntervalMs = v))} />
        </Row>
        <Row title="Minimum text" help="Characters needed before the first suggestion.">
          <Slider label="Minimum text" value={c.minChars} min={4} max={80} step={1} format={(v) => `${v} chars`} onCommit={(v) => update((s) => void (s.completion.minChars = v))} />
        </Row>
        <Row title="Suggestion length" help="Upper bound on suggested words.">
          <Slider label="Suggestion length" value={c.maxWords} min={3} max={30} step={1} format={(v) => `${v} words`} onCommit={(v) => update((s) => void (s.completion.maxWords = v))} />
        </Row>
      </Card>
    </Page>
  );
}

export function WritingSection() {
  const { settings, update } = useSettings();
  const [word, setWord] = useState("");
  if (!settings) return null;
  const w = settings.writing;
  const addWord = () => {
    const value = word.trim().toLowerCase();
    if (!value || w.ignoredWords.includes(value)) return;
    // Keep what was typed if saving fails (the error is shown above the page).
    void update((s) => void (s.writing.ignoredWords = [...s.writing.ignoredWords, value].sort())).then(
      (saved) => saved && setWord(""),
    );
  };
  return (
    <Page title="Writing" subtitle="Spelling and grammar help that fixes mistakes in place without rewriting what you meant.">
      <Card>
        <ToggleRow title="Writing assistance" checked={w.enabled} onChange={(v) => update((s) => void (s.writing.enabled = v))} />
        <ToggleRow
          title="Spelling suggestions"
          help="Runs on your Mac with a built-in dictionary. Uses no tokens."
          checked={w.spelling}
          onChange={(v) => update((s) => void (s.writing.spelling = v))}
          disabled={!w.enabled}
        />
        <ToggleRow
          title="Grammar check with AI"
          help="When a finished sentence looks wrong (“we dont have…”), Mote asks the writing model for a minimal fix."
          checked={w.aiGrammar}
          onChange={(v) => update((s) => void (s.writing.aiGrammar = v))}
          disabled={!w.enabled}
        />
      </Card>
      <div style={{ marginTop: 16 }}>
        <Note icon="globe">
          Hinglish and Marathi written in English letters are never “corrected” into English. Words like <em>bhai</em>, <em>kal</em>,{" "}
          <em>karaycha</em> and <em>aahe</em> are recognized, and AI rewrites keep your language and script unless you ask to translate.
        </Note>
      </div>

      <div className="section-label">Words to accept</div>
      <Card subtitle="Mote won't flag these. Pressing Esc on a spelling suggestion also ignores that word for the session.">
        <div className="card-body" style={{ display: "flex", gap: 8 }}>
          <input
            className="input"
            aria-label="Word to accept"
            placeholder="Add a word, name or term"
            value={word}
            onChange={(e) => setWord(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addWord()}
            style={{ flex: 1 }}
          />
          <button type="button" className="btn" onClick={addWord} disabled={!word.trim()}>
            <Icon name="plus" size={14} />
            Add
          </button>
        </div>
        {w.ignoredWords.length ? (
          <div style={{ padding: "0 18px 16px", display: "flex", flexWrap: "wrap", gap: 6 }}>
            {w.ignoredWords.map((iw) => (
              <span key={iw} className="badge">
                {iw}
                <button
                  type="button"
                  className="btn ghost small"
                  style={{ height: 18, padding: "0 2px" }}
                  aria-label={`Remove ${iw}`}
                  onClick={() => update((s) => void (s.writing.ignoredWords = s.writing.ignoredWords.filter((x) => x !== iw)))}
                >
                  <Icon name="x" size={12} />
                </button>
              </span>
            ))}
          </div>
        ) : null}
      </Card>
    </Page>
  );
}

export function ContextSection() {
  const { settings, update } = useSettings();
  if (!settings) return null;
  const ctx = settings.context;
  return (
    <Page title="Context" subtitle="Mote notices what you're doing (which app, what you just copied) so it can offer the right help without being asked.">
      <Card>
        <ToggleRow
          title="Contextual suggestions"
          help="Example: copy an email in Gmail, switch to your editor's AI chat, and Mote offers “Create coding task” from the copied text. Nothing is sent until you choose an action."
          checked={ctx.contextualSuggestions}
          onChange={(v) => update((s) => void (s.context.contextualSuggestions = v))}
        />
        <Row title="Remember copied text for" help="Copied text is kept in memory only, never on disk, and is forgotten after this time.">
          <select
            className="select"
            aria-label="Remember copied text for"
            value={ctx.clipboardTtlSecs}
            onChange={(e) => update((s) => void (s.context.clipboardTtlSecs = Number(e.target.value)))}
          >
            {[60, 180, 600, 1800].map((secs) => (
              <option key={secs} value={secs}>
                {secs < 120 ? `${secs} seconds` : `${secs / 60} minutes`}
              </option>
            ))}
          </select>
        </Row>
        <ToggleRow
          title="AI classification when unsure"
          help="If local signals can't tell a message from an AI prompt, Mote sends a short excerpt (≤400 characters) to the classification model once per field."
          checked={ctx.aiClassification}
          onChange={(v) => update((s) => void (s.context.aiClassification = v))}
        />
        <ToggleRow
          title="Prompt enhancement hints"
          help="In AI prompt boxes, show a small “Enhance prompt” hint once per field."
          checked={settings.prompts.showHint}
          onChange={(v) => update((s) => void (s.prompts.showHint = v))}
        />
      </Card>
      <div style={{ marginTop: 16 }}>
        <Note>
          Mote decides between <strong>conversation</strong>, <strong>AI prompt</strong>, code, command, note, search and form using the app, the field&apos;s placeholder, the text itself, its language, and recent app switches. Most decisions never leave your computer.
        </Note>
      </div>
    </Page>
  );
}
