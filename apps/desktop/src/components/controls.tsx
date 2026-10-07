import { useEffect, useId, useState, type ReactNode } from "react";

import { Icon, type IconName } from "./Icon";

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    />
  );
}

/** A settings row: title + help on the left, a control on the right. */
export function Row({ title, help, children, htmlFor }: { title: ReactNode; help?: ReactNode; children?: ReactNode; htmlFor?: string }) {
  return (
    <div className="row">
      <div className="row-text">
        <div className="row-title">{htmlFor ? <label htmlFor={htmlFor}>{title}</label> : title}</div>
        {help ? <div className="row-help">{help}</div> : null}
      </div>
      {children ? <div className="row-control">{children}</div> : null}
    </div>
  );
}

export function ToggleRow({
  title,
  help,
  checked,
  onChange,
  disabled,
}: {
  title: string;
  help?: ReactNode;
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <Row title={title} help={help}>
      <Switch checked={checked} onChange={onChange} label={title} disabled={disabled} />
    </Row>
  );
}

export function Card({ title, subtitle, actions, children, className }: {
  title?: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`card ${className ?? ""}`}>
      {title || actions ? (
        <div className="card-header">
          <div>
            {title ? <h2 className="card-title">{title}</h2> : null}
            {subtitle ? <p className="card-subtitle">{subtitle}</p> : null}
          </div>
          {actions ? <div className="row-control">{actions}</div> : null}
        </div>
      ) : null}
      {children}
    </section>
  );
}

export function Page({ title, subtitle, children, wide }: { title: string; subtitle?: ReactNode; children: ReactNode; wide?: boolean }) {
  return (
    <div className={`page ${wide ? "wide" : ""}`}>
      <header className="page-header">
        <h1 className="page-title">{title}</h1>
        {subtitle ? <p className="page-subtitle">{subtitle}</p> : null}
      </header>
      {children}
    </div>
  );
}

export function Note({ children, tone, icon = "info" }: { children: ReactNode; tone?: "danger" | "accent"; icon?: IconName }) {
  return (
    <div className="inline-note" data-tone={tone} role={tone === "danger" ? "alert" : undefined}>
      <Icon name={icon} />
      <div>{children}</div>
    </div>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** A number slider that commits when released. */
export function Slider({
  value,
  min,
  max,
  step,
  onCommit,
  format,
  label,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  onCommit: (value: number) => void;
  format: (value: number) => string;
  label: string;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const id = useId();
  return (
    <div className="slider-wrap">
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={draft}
        aria-label={label}
        onChange={(e) => setDraft(Number(e.target.value))}
        onPointerUp={() => draft !== value && onCommit(draft)}
        onKeyUp={() => draft !== value && onCommit(draft)}
        onBlur={() => draft !== value && onCommit(draft)}
      />
      <output htmlFor={id} className="slider-value">
        {format(draft)}
      </output>
    </div>
  );
}

/** A text input that commits on blur or Enter. */
export function CommitInput({
  value,
  onCommit,
  label,
  className,
  placeholder,
  type = "text",
  invalid,
  width,
}: {
  value: string;
  onCommit: (value: string) => void;
  label: string;
  className?: string;
  placeholder?: string;
  type?: string;
  invalid?: boolean;
  width?: number;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => draft !== value && onCommit(draft.trim());
  return (
    <input
      className={`input ${className ?? ""}`}
      aria-label={label}
      aria-invalid={invalid || undefined}
      value={draft}
      type={type}
      placeholder={placeholder}
      style={width ? { width } : undefined}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") setDraft(value);
      }}
    />
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

export function Toast({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <div className="toast" role="status">
      {message}
    </div>
  );
}
