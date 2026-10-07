import { useEffect, useState } from "react";

import type { EngineStatus } from "../bindings/EngineStatus";
import { api, subscribe } from "../lib/ipc";

/** Live engine status (from the backend's status events). */
export function useEngineStatus(): EngineStatus | null {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .engineStatus()
      .then((s) => alive && setStatus(s))
      .catch(() => undefined);
    const unlisten = subscribe<EngineStatus>("engine-status", (s) => setStatus(s));
    return () => {
      alive = false;
      void unlisten.then((fn) => fn());
    };
  }, []);
  return status;
}

export const STATE_TEXT: Record<EngineStatus["state"], string> = {
  starting: "Starting…",
  active: "Active",
  idle: "Active",
  paused: "Paused",
  disabled: "Assistance off",
  excluded: "Excluded app",
  needs_permission: "Needs permission",
  secure_input: "Secure input active",
  needs_api_key: "Needs API key",
  cloud_disabled: "Cloud AI off",
  offline: "Offline",
  rate_limited: "Rate limited",
};

export function stateTone(state: EngineStatus["state"]): "good" | "warning" | "neutral" {
  switch (state) {
    case "active":
    case "idle":
      return "good";
    case "needs_permission":
    case "needs_api_key":
    case "offline":
    case "rate_limited":
      return "warning";
    default:
      return "neutral";
  }
}
