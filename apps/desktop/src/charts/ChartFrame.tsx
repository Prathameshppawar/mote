import { useState, type ReactNode } from "react";

export type TableRow = { label: string; value: string };

/**
 * A chart card with title, subtitle and a table-view toggle. The table is the
 * accessible twin of every chart: every value is reachable without hovering.
 */
export function ChartFrame({
  title,
  subtitle,
  table,
  valueHeader = "Value",
  children,
}: {
  title: string;
  subtitle?: ReactNode;
  table: TableRow[];
  valueHeader?: string;
  children: ReactNode;
}) {
  const [showTable, setShowTable] = useState(false);
  return (
    <figure className="card chart-card">
      <div className="card-header">
        <div>
          <figcaption className="card-title">{title}</figcaption>
          {subtitle ? <p className="card-subtitle">{subtitle}</p> : null}
        </div>
        <button type="button" className="btn ghost small" aria-pressed={showTable} onClick={() => setShowTable((v) => !v)}>
          {showTable ? "Chart" : "Table"}
        </button>
      </div>
      <div className="chart-body">
        {showTable ? (
          <div className="chart-table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th scope="col">Period</th>
                  <th scope="col">{valueHeader}</th>
                </tr>
              </thead>
              <tbody>
                {table.map((row) => (
                  <tr key={row.label}>
                    <td>{row.label}</td>
                    <td>{row.value}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          children
        )}
      </div>
    </figure>
  );
}
