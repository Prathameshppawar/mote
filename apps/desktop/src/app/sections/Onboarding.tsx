import { useCallback, useEffect, useState, type ReactNode } from "react";

import type { HealthReport } from "../../bindings/HealthReport";
import type { PermissionStatus } from "../../bindings/PermissionStatus";
import { Kbd, Note } from "../../components/controls";
import { Icon, Logo, type IconName } from "../../components/Icon";
import { api, errorMessage } from "../../lib/ipc";
import { openLink } from "../../lib/links";
import { shortcutLabel } from "../../lib/shortcuts";
import { useSettings } from "../settingsContext";
import { HealthLine } from "./Providers";

const STEPS = ["welcome", "privacy", "permission", "key", "shortcuts"] as const;
type Step = (typeof STEPS)[number];

function Feature({ icon, title, children }: { icon: IconName; title: string; children: ReactNode }) {
  return (
    <li>
      <span className="feature-icon">
        <Icon name={icon} />
      </span>
      <div>
        <div style={{ fontWeight: 600 }}>{title}</div>
        <div className="muted">{children}</div>
      </div>
    </li>
  );
}

function PermissionStep({ onReady }: { onReady: (granted: boolean) => void }) {
  const [status, setStatus] = useState<PermissionStatus | null>(null);
  useEffect(() => {
    let alive = true;
    const poll = () =>
      api.permissionStatus().then(
        (s) => {
          if (!alive) return;
          setStatus(s);
          onReady(s.accessibility === "granted" || s.accessibility === "not_required");
        },
        () => undefined,
      );
    void poll();
    const timer = setInterval(poll, 1500);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [onReady]);

  if (status?.accessibility === "not_required") {
    return (
      <>
        <h1>No permission needed</h1>
        <p className="lead">Windows lets Mote read the focused text field through UI Automation without a separate permission.</p>
        <div style={{ marginTop: 18 }}>
          <Note icon="info">Some apps running as administrator can&apos;t be assisted unless Mote also runs as administrator. Mote never asks for that.</Note>
        </div>
      </>
    );
  }
  const granted = status?.accessibility === "granted";
  return (
    <>
      <h1>Allow Accessibility access</h1>
      <p className="lead">
        macOS asks you to approve apps that read text fields and type for you. Mote uses this to see the text around your cursor, place suggestions
        next to it, and insert them when you press Tab.
      </p>
      <ul className="feature-list">
        <Feature icon="check" title="What Mote can access">
          The focused text field, its position on screen, and which app is in front. Never password fields.
        </Feature>
        <Feature icon="general" title="How to enable it">
          Click the button, then turn on <strong>Mote</strong> in System Settings → Privacy &amp; Security → Accessibility.
        </Feature>
        <Feature icon="x" title="How to turn it off">
          Switch Mote off in the same list at any time, or pause Mote from the menu bar.
        </Feature>
      </ul>
      <div style={{ marginTop: 22, display: "flex", gap: 10, alignItems: "center" }}>
        {granted ? (
          <span className="badge" data-tone="good">
            <Icon name="check" size={12} /> Access granted
          </span>
        ) : (
          <>
            <button
              type="button"
              className="btn primary"
              onClick={async () => {
                await api.requestAccessibility().catch(() => undefined);
                await api.openAccessibilitySettings().catch(() => undefined);
              }}
            >
              Open System Settings
            </button>
            <span className="muted" style={{ fontSize: 12.5 }}>
              Waiting for permission…
            </span>
          </>
        )}
      </div>
    </>
  );
}

function KeyStep({ onReady }: { onReady: (ok: boolean) => void }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<HealthReport | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api.providerStatus().then(
      (s) => {
        if (s.hasApiKey) {
          setSaved(true);
          onReady(true);
        }
      },
      () => undefined,
    );
  }, [onReady]);

  const save = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const status = await api.setApiKey(key);
      setReport(status.health);
      setSaved(true);
      setKey("");
      onReady(true);
    } catch (e) {
      setProblem(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <h1>Connect Groq</h1>
      <p className="lead">
        Mote uses Groq for fast, inexpensive AI. Create a free API key at{" "}
        <a href="https://console.groq.com/keys" onClick={openLink}>
          console.groq.com/keys
        </a>{" "}
        and paste it here. It is stored in your system keychain, never in Mote&apos;s files.
      </p>
      <div style={{ display: "flex", gap: 8, marginTop: 22 }}>
        <input
          className="input mono"
          type="password"
          aria-label="Groq API key"
          placeholder={saved ? "Key saved — paste to replace" : "gsk_…"}
          value={key}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && key && save()}
          style={{ flex: 1, height: 38 }}
          autoFocus
        />
        <button type="button" className="btn primary large" disabled={!key || busy} onClick={save}>
          {busy ? <span className="spinner" /> : null}
          Save &amp; test
        </button>
      </div>
      <div style={{ marginTop: 14 }}>
        {problem ? (
          <Note tone="danger" icon="alert">
            {problem}
          </Note>
        ) : report ? (
          <HealthLine report={report} />
        ) : saved ? (
          <Note icon="check">A key is saved. You can continue.</Note>
        ) : (
          <Note icon="lock">Groq&apos;s free tier is enough to try Mote. Usage appears in Mote&apos;s Usage dashboard.</Note>
        )}
      </div>
    </>
  );
}

export function Onboarding({ onDone }: { onDone: () => void }) {
  const { settings, replace } = useSettings();
  const [step, setStep] = useState<Step>("welcome");
  const [ready, setReady] = useState<Record<string, boolean>>({});
  const index = STEPS.indexOf(step);
  const next = STEPS[index + 1];
  const previous = STEPS[index - 1];
  const keyboard = settings?.keyboard;

  const finish = async () => {
    try {
      replace(await api.completeOnboarding());
    } catch {
      // Continue anyway; the setting can be completed later.
    }
    onDone();
  };

  const canContinue = step !== "permission" || Boolean(ready.permission);
  const onPermission = useCallback(
    (granted: boolean) => setReady((r) => (r.permission === granted ? r : { ...r, permission: granted })),
    [],
  );
  const onKey = useCallback((ok: boolean) => setReady((r) => (r.key === ok ? r : { ...r, key: ok })), []);

  return (
    <div className="onboarding">
      <div className="card onboarding-card">
        <div className="onboarding-steps" aria-label={`Step ${index + 1} of ${STEPS.length}`}>
          {STEPS.map((s, i) => (
            <span key={s} data-done={i <= index} />
          ))}
        </div>

        {step === "welcome" ? (
          <>
            <Logo size={52} />
            <h1 style={{ marginTop: 16 }}>Welcome to Mote</h1>
            <p className="lead">AI that stays in the flow. Mote helps where you already write, without switching apps.</p>
            <ul className="feature-list">
              <Feature icon="completion" title="Inline completion">
                Short continuations as you type. Press Tab to accept.
              </Feature>
              <Feature icon="writing" title="Writing help that keeps your voice">
                Fixes spelling and grammar in place, including in Hinglish and romanized Marathi, without translating.
              </Feature>
              <Feature icon="sparkle" title="Better prompts">
                Recognizes AI prompt boxes and offers to make prompts clearer and more precise.
              </Feature>
              <Feature icon="clipboard" title="Context-aware">
                Copy an email, switch to your editor, and Mote offers to turn it into a task.
              </Feature>
            </ul>
          </>
        ) : null}

        {step === "privacy" ? (
          <>
            <h1>Your data stays yours</h1>
            <p className="lead">Mote is local-first. Here is the whole story in four lines.</p>
            <ul className="feature-list">
              <Feature icon="activity" title="What Mote observes">
                The app in front, the text near your cursor in the focused field, and recently copied text. In memory only.
              </Feature>
              <Feature icon="globe" title="What can be sent to Groq">
                Only the text a feature needs, when it runs: a few hundred characters for a completion, a finished sentence for a grammar check, or what
                you choose in the command palette.
              </Feature>
              <Feature icon="lock" title="What stays on your computer">
                Settings, usage statistics (no text) and an activity log (no text) in a local database you can clear any time.
              </Feature>
              <Feature icon="excluded" title="What Mote never touches">
                Password managers, password fields, apps you exclude, and anything while you pause Mote.
              </Feature>
            </ul>
          </>
        ) : null}

        {step === "permission" ? <PermissionStep onReady={onPermission} /> : null}

        {step === "key" ? <KeyStep onReady={onKey} /> : null}

        {step === "shortcuts" ? (
          <>
            <h1>You&apos;re set</h1>
            <p className="lead">Mote lives in your menu bar. Start typing in Slack, Mail, ChatGPT or anywhere else.</p>
            <div className="shortcut-grid">
              <Kbd>Tab</Kbd>
              <span>Accept a suggestion</span>
              <Kbd>Esc</Kbd>
              <span>Dismiss it</span>
              <Kbd>{keyboard ? shortcutLabel(keyboard.nextSuggestion) : "⌥]"}</Kbd>
              <span>Show another suggestion</span>
              <Kbd>{keyboard ? shortcutLabel(keyboard.commandPalette) : "⌘⇧Space"}</Kbd>
              <span>Command palette: improve, rewrite, translate, enhance a prompt, use copied text</span>
            </div>
            {!ready.key ? (
              <div style={{ marginTop: 18 }}>
                <Note icon="alert">No API key yet: local spelling works, AI features start once you add a key in Settings → AI Providers.</Note>
              </div>
            ) : null}
          </>
        ) : null}

        <div className="onboarding-footer">
          {previous ? (
            <button type="button" className="btn ghost" onClick={() => setStep(previous)}>
              Back
            </button>
          ) : (
            <span />
          )}
          {next ? (
            <div style={{ display: "flex", gap: 8 }}>
              {step === "permission" && !ready.permission ? (
                <button type="button" className="btn ghost" onClick={() => setStep(next)}>
                  Skip for now
                </button>
              ) : null}
              {step === "key" && !ready.key ? (
                <button type="button" className="btn ghost" onClick={() => setStep(next)}>
                  Add later
                </button>
              ) : null}
              <button type="button" className="btn primary large" disabled={!canContinue || (step === "key" && !ready.key)} onClick={() => setStep(next)}>
                Continue
              </button>
            </div>
          ) : (
            <button type="button" className="btn primary large" onClick={finish}>
              Start using Mote
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
