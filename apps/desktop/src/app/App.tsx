import { useEffect, useState, type ReactNode } from "react";

import { Icon, Logo, type IconName } from "../components/Icon";
import { inTauri, subscribe } from "../lib/ipc";
import { useSettings } from "./settingsContext";
import { STATE_TEXT, stateTone, useEngineStatus } from "./engineStatus";
import { SettingsProvider } from "./settings";
import { About } from "./sections/About";
import { CompletionSection, ContextSection, WritingSection } from "./sections/Assistance";
import { Diagnostics } from "./sections/Diagnostics";
import { ExcludedApps } from "./sections/Excluded";
import { General } from "./sections/General";
import { Keyboard } from "./sections/Keyboard";
import { Models } from "./sections/Models";
import { Onboarding } from "./sections/Onboarding";
import { Privacy } from "./sections/Privacy";
import { Providers } from "./sections/Providers";
import { Usage } from "./sections/Usage";

type Section = { id: string; label: string; icon: IconName; render: () => ReactNode };

const SECTIONS: Section[] = [
  { id: "general", label: "General", icon: "general", render: () => <General /> },
  { id: "providers", label: "AI Providers", icon: "key", render: () => <Providers /> },
  { id: "models", label: "Models", icon: "models", render: () => <Models /> },
  { id: "completion", label: "Completion", icon: "completion", render: () => <CompletionSection /> },
  { id: "writing", label: "Writing", icon: "writing", render: () => <WritingSection /> },
  { id: "context", label: "Context", icon: "context", render: () => <ContextSection /> },
  { id: "privacy", label: "Privacy", icon: "privacy", render: () => <Privacy /> },
  { id: "usage", label: "Usage", icon: "usage", render: () => <Usage /> },
  { id: "keyboard", label: "Keyboard", icon: "keyboard", render: () => <Keyboard /> },
  { id: "excluded", label: "Excluded Apps", icon: "excluded", render: () => <ExcludedApps /> },
  { id: "diagnostics", label: "Diagnostics", icon: "diagnostics", render: () => <Diagnostics /> },
  { id: "about", label: "About", icon: "about", render: () => <About /> },
];

const ALIASES: Record<string, string> = { activity: "privacy", "": "general" };

function currentRoute(): string {
  const raw = window.location.hash.replace(/^#\/?/, "");
  return ALIASES[raw] ?? raw;
}

function useRoute(): [string, (route: string) => void] {
  const [route, setRoute] = useState(currentRoute);
  useEffect(() => {
    const onHash = () => setRoute(currentRoute());
    window.addEventListener("hashchange", onHash);
    const unlisten = subscribe<string>("navigate", (section) => {
      window.location.hash = `#/${section}`;
    });
    return () => {
      window.removeEventListener("hashchange", onHash);
      void unlisten.then((fn) => fn());
    };
  }, []);
  return [route, (r) => (window.location.hash = `#/${r}`)];
}

function Sidebar({ route, navigate }: { route: string; navigate: (r: string) => void }) {
  const status = useEngineStatus();
  return (
    <nav className="sidebar" aria-label="Settings">
      <div className="brand">
        <Logo />
        <div>
          <div className="brand-name">Mote</div>
          <div className="brand-tagline">AI that stays in the flow</div>
        </div>
      </div>
      <div className="nav">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            type="button"
            className="nav-item"
            aria-current={route === s.id ? "page" : undefined}
            onClick={() => navigate(s.id)}
          >
            <Icon name={s.icon} />
            {s.label}
          </button>
        ))}
      </div>
      <div className="sidebar-footer">
        {status ? (
          <span className="status-pill" title={status.message ?? undefined}>
            <span className="status-dot" data-tone={stateTone(status.state)} />
            {STATE_TEXT[status.state]}
          </span>
        ) : null}
        {!inTauri() ? <p className="muted" style={{ fontSize: 11.5, marginTop: 8 }}>Browser preview · sample data</p> : null}
      </div>
    </nav>
  );
}

function Shell() {
  const { settings } = useSettings();
  const [route, navigate] = useRoute();
  if (!settings) {
    return (
      <div className="onboarding">
        <span className="spinner" aria-label="Loading" />
      </div>
    );
  }
  if (route === "onboarding" || !settings.general.onboardingCompleted) {
    return <Onboarding onDone={() => navigate("general")} />;
  }
  const section = SECTIONS.find((s) => s.id === route) ?? SECTIONS[0];
  return (
    <div className="shell">
      <Sidebar route={section?.id ?? "general"} navigate={navigate} />
      <main className="content" key={section?.id}>
        {section?.render()}
      </main>
    </div>
  );
}

export function App() {
  return (
    <SettingsProvider>
      <Shell />
    </SettingsProvider>
  );
}
