import { useLayoutEffect, useRef, useState } from "react";

/**
 * Tracks an element's content width. Measures synchronously before the first
 * paint (so charts never render at a stale size), then follows resizes.
 */
export function useWidth<T extends HTMLElement>(fallback = 600) {
  const ref = useRef<T | null>(null);
  const [width, setWidth] = useState(fallback);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const w = el.getBoundingClientRect().width;
      if (w > 0) setWidth((current) => (Math.abs(current - w) > 0.5 ? w : current));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return [ref, width] as const;
}
