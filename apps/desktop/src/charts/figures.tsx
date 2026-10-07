import type { ReactNode } from "react";

import { Icon } from "../components/Icon";

/** Stat tile: label · value · supporting line. */
export function StatTile({ label, value, sub, hero }: { label: string; value: string; sub?: ReactNode; hero?: boolean }) {
  return (
    <div className={`stat-tile ${hero ? "hero" : ""}`}>
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      {sub ? <div className="stat-sub">{sub}</div> : null}
    </div>
  );
}

/**
 * Meter for a ratio against a limit. The track is a lighter step of the fill's
 * ramp; low remaining capacity switches to a status color with an icon and a
 * label, so state never relies on color alone.
 */
export function Meter({ label, used, limit, detail }: { label: string; used: number; limit: number; detail: string }) {
  const remaining = Math.max(0, limit - used);
  const ratio = limit > 0 ? Math.min(1, used / limit) : 0;
  const remainingRatio = limit > 0 ? remaining / limit : 1;
  const status = remainingRatio <= 0.05 ? "critical" : remainingRatio <= 0.2 ? "warning" : null;
  return (
    <div className="meter">
      <div className="meter-head">
        <span className="meter-label">{label}</span>
        {status ? (
          <span className="meter-status" data-status={status}>
            <Icon name="alert" size={13} />
            {status === "critical" ? "Almost exhausted" : "Running low"}
          </span>
        ) : null}
      </div>
      <div
        className="meter-track"
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={limit}
        aria-valuenow={used}
      >
        <div className="meter-fill" data-status={status ?? undefined} style={{ width: `${ratio * 100}%` }} />
      </div>
      <div className="meter-detail">{detail}</div>
    </div>
  );
}
