import { useState } from "react";

export type BarItem = { key: string; label: string; value: number; valueLabel: string; detail: string };

/**
 * Horizontal bars for nominal categories: one series, so every bar takes the
 * same slot-1 hue; length carries magnitude and the value sits at the tip.
 */
export function BarList({ items, ariaLabel }: { items: BarItem[]; ariaLabel: string }) {
  const [active, setActive] = useState<string | null>(null);
  const max = Math.max(1, ...items.map((i) => i.value));
  return (
    <ul className="bar-list" aria-label={ariaLabel}>
      {items.map((item) => {
        const pct = (item.value / max) * 100;
        return (
          <li
            key={item.key}
            className="bar-row"
            tabIndex={0}
            data-active={active === item.key || undefined}
            onPointerEnter={() => setActive(item.key)}
            onPointerLeave={() => setActive(null)}
            onFocus={() => setActive(item.key)}
            onBlur={() => setActive(null)}
            aria-label={`${item.label}: ${item.valueLabel}. ${item.detail}`}
          >
            <span className="bar-label">{item.label}</span>
            <span className="bar-track" aria-hidden="true">
              <span className="bar-fill" style={{ width: `${Math.max(pct, 0.5)}%` }} />
            </span>
            <span className="bar-value">{item.valueLabel}</span>
            {active === item.key ? (
              <span className="bar-tooltip" role="status">
                {item.detail}
              </span>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}
