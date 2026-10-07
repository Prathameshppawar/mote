import type { MouseEvent } from "react";

import { openUrl } from "@tauri-apps/plugin-opener";

import { inTauri } from "./ipc";

/** Opens http(s) links in the system browser instead of the app window. */
export function openLink(event: MouseEvent<HTMLAnchorElement>): void {
  const href = event.currentTarget.href;
  if (!/^https?:\/\//.test(href)) return;
  event.preventDefault();
  if (inTauri()) {
    void openUrl(href);
  } else {
    window.open(href, "_blank", "noopener,noreferrer");
  }
}
