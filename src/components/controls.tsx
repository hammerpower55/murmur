// Small form controls shared by Settings, Dictionary and Snippets.

import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Hang } from "./Page";

/** Words in a row with a soft pill behind the selected one that slides across. */
export function Tabs<T extends string>({
  tabs,
  value,
  onChange,
}: {
  tabs: [T, string][];
  value: T;
  onChange: (t: T) => void;
}) {
  const row = useRef<HTMLDivElement>(null);
  const [mark, setMark] = useState<{ x: number; w: number } | null>(null);
  const [placed, setPlaced] = useState(false);
  useLayoutEffect(() => {
    const el = row.current?.querySelector<HTMLElement>(`[data-tab="${value}"]`);
    if (el) setMark({ x: el.offsetLeft, w: el.offsetWidth });
  }, [value]);
  useLayoutEffect(() => {
    if (mark && !placed) requestAnimationFrame(() => setPlaced(true));
  }, [mark, placed]);
  return (
    <div ref={row} role="tablist" className="relative flex w-fit items-center gap-1 rounded-full bg-surface p-1">
      {mark && (
        <span
          aria-hidden
          className="absolute top-1 bottom-1 left-0 rounded-full bg-surface-2"
          style={{
            width: mark.w,
            transform: `translateX(${mark.x}px)`,
            transition: placed ? "transform 340ms var(--ease-out-quint), width 340ms var(--ease-out-quint)" : "none",
          }}
        />
      )}
      {tabs.map(([id, label]) => (
        <button
          key={id}
          role="tab"
          data-tab={id}
          aria-selected={value === id}
          onClick={() => onChange(id)}
          className={`relative rounded-full px-4 py-1.5 ${value === id ? "font-semibold text-ink" : "text-ink-2 hover:text-ink"}`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Hang label={title} className="mt-8 first-of-type:mt-0">
      <div className="rows">{children}</div>
    </Hang>
  );
}

/** One setting per row: label on the left, control on the right. */
export function Row({ label, children, htmlFor }: { label: string; children: ReactNode; htmlFor?: string }) {
  return (
    <div className="flex min-h-12 items-center justify-between gap-6 py-2">
      <label htmlFor={htmlFor}>{label}</label>
      <div className="flex shrink-0 items-center gap-3">{children}</div>
    </div>
  );
}

/** A pill with a round knob that slides across. */
export function Toggle({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (on: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-10 shrink-0 rounded-full disabled:opacity-35 ${
        checked ? "bg-[var(--switch-on)]" : "bg-surface-2"
      }`}
    >
      <span
        className={`absolute top-1 left-1 h-4 w-4 rounded-full transition-[transform,background-color] duration-300 ease-[var(--ease-out-quint)] ${
          checked ? "translate-x-4 bg-[var(--switch-dot)]" : "bg-ink-2"
        }`}
      />
    </button>
  );
}

export const selectClass = "field field-select";
export const buttonClass = "btn";
export const primaryClass = "btn-primary";
export const linkClass = "link";
export const inputClass = "field";

export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  label,
  format,
  disabled,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  label: string;
  format: (v: number) => string;
  disabled?: boolean;
}) {
  return (
    <>
      <input
        type="range"
        aria-label={label}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
        className="range"
      />
      <span className="w-10 text-right text-ink-2 tabular-nums">{format(value)}</span>
    </>
  );
}

/** Key caps for a hotkey. */
export function Keys({ names, label }: { names: string[]; label: (n: string) => string }) {
  return (
    <span className="flex gap-1">
      {names.map((n) => (
        <kbd key={n} className="rounded-lg bg-surface-2 px-2 py-0.5 font-sans text-caption font-semibold">
          {label(n)}
        </kbd>
      ))}
    </span>
  );
}
