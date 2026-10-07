import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";

import type { CommandError } from "../bindings/CommandError";
import type { Settings } from "../bindings/Settings";
import { api, subscribe, toCommandError } from "../lib/ipc";
import { SettingsContext, type SettingsContextValue } from "./settingsContext";

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<CommandError | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .settings()
      .then((s) => alive && setSettings(s))
      .catch((e) => alive && setError(toCommandError(e)));
    const unlisten = subscribe<Settings>("settings-changed", (s) => setSettings(s));
    return () => {
      alive = false;
      void unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const theme = settings?.general.theme;
    const root = document.documentElement;
    if (!theme || theme === "system") delete root.dataset.theme;
    else root.dataset.theme = theme;
  }, [settings?.general.theme]);

  const update = useCallback(
    async (mutate: (draft: Settings) => void) => {
      if (!settings) return false;
      const previous = settings;
      const next = structuredClone(settings);
      mutate(next);
      setSettings(next);
      try {
        setSettings(await api.saveSettings(next));
        setError(null);
        return true;
      } catch (e) {
        setSettings(previous);
        setError(toCommandError(e));
        return false;
      }
    },
    [settings],
  );

  const clearError = useCallback(() => setError(null), []);

  const value = useMemo<SettingsContextValue>(
    () => ({
      settings,
      update,
      replace: setSettings,
      error,
      fieldError: (path) => error?.fields.find((f) => f.field === path)?.message,
      clearError,
    }),
    [settings, update, error, clearError],
  );

  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}
