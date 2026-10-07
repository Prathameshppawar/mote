import { useEffect, useLayoutEffect, useRef, useState } from "react";

import type { OverlayPayload } from "../bindings/OverlayPayload";
import { Icon } from "../components/Icon";
import { api, inTauri, subscribe } from "../lib/ipc";

/** Margin kept around the pill so its shadow is not clipped by the window. */
const MARGIN = 8;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function Overlay() {
  const [payload, setPayload] = useState<OverlayPayload | null>(null);
  const pill = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // Browser preview only: overlay.html?demo=completion renders a sample.
    const demo = new URLSearchParams(window.location.search).get("demo");
    if (!inTauri() && demo) {
      void import("../lib/mock").then((m) => setPayload(m.sampleOverlay(demo)));
    }
    const view = subscribe<OverlayPayload>("overlay-view", setPayload);
    const hide = subscribe("overlay-hide", () => setPayload(null));
    return () => {
      void view.then((fn) => fn());
      void hide.then((fn) => fn());
    };
  }, []);

  // Report the rendered size so the native window fits the content exactly.
  useLayoutEffect(() => {
    if (!payload || !pill.current) return;
    const rect = pill.current.getBoundingClientRect();
    void api.overlayReady(payload.seq, Math.ceil(rect.width) + MARGIN * 2, Math.ceil(rect.height) + MARGIN * 2).catch(() => undefined);
  }, [payload]);

  const view = payload?.view;
  // Caret rectangles are in physical pixels on Windows; CSS sizes are logical.
  const anchorHeight = view?.anchor?.height ?? 18;
  const lineHeight = view?.coordinateSpace === "physical_pixels" ? anchorHeight / (window.devicePixelRatio || 1) : anchorHeight;
  const fontSize = view?.kind === "completion" ? clamp(lineHeight * 0.74, 12, 20) : 13;
  const [from, to] = (view?.detail ?? "").split(" → ");

  return (
    <div className="overlay-stage" style={{ padding: MARGIN }}>
      {/* The live region stays mounted so each new suggestion is announced. */}
      <div role="status" aria-live="polite">
        {view ? (
          <div ref={pill} className={`pill pill-${view.kind}`} style={{ fontSize }}>
            {view.kind === "completion" ? (
              <>
                <span className="ghost">{view.text.replace(/^ /, " ")}</span>
                {view.count > 1 ? (
                  <span className="counter">
                    {view.index + 1}/{view.count}
                  </span>
                ) : null}
                <kbd className="key">Tab</kbd>
              </>
            ) : null}

            {view.kind === "correction" ? (
              <>
                <Icon name="writing" size={13} />
                <span className="fix">
                  <s>{from}</s>
                  <span className="arrow">→</span>
                  <strong>{to}</strong>
                </span>
                <kbd className="key">Tab</kbd>
              </>
            ) : null}

            {view.kind === "prompt_hint" ? (
              <>
                <Icon name="sparkle" size={13} />
                <span>{view.text}</span>
                {view.detail ? <kbd className="key">{view.detail}</kbd> : null}
              </>
            ) : null}

            {view.kind === "context" ? (
              <>
                <Icon name="clipboard" size={13} />
                <span className="context-text">
                  <strong>{view.text}</strong>
                  {view.detail ? <span className="context-detail">{view.detail}</span> : null}
                </span>
              </>
            ) : null}

            {view.kind === "notice" ? (
              <>
                <Icon name="check" size={13} />
                <span>{view.text}</span>
              </>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}
