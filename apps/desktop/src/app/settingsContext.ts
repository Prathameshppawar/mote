import { createContext, useContext } from "react";

import type { CommandError } from "../bindings/CommandError";
import type { Settings } from "../bindings/Settings";

export type SettingsContextValue = {
  settings: Settings | null;
  /** Applies `mutate` to a copy, saves it, and reverts on failure. */
  update: (mutate: (draft: Settings) => void) => Promise<boolean>;
  replace: (settings: Settings) => void;
  error: CommandError | null;
  fieldError: (path: string) => string | undefined;
  clearError: () => void;
};

export const SettingsContext = createContext<SettingsContextValue | null>(null);

export function useSettings(): SettingsContextValue {
  const ctx = useContext(SettingsContext);
  if (!ctx) throw new Error("useSettings must be used inside SettingsProvider");
  return ctx;
}
