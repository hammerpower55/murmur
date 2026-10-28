// Small form controls shared by Settings, Dictionary and Snippets.

import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

/** Segmented tabs with a pill that slides to the selected one. */
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
  const [pill, setPill] = useState<{ x: number; w: number } | null>(null);
  const [placed, setPlaced] = useState(false);
  useLayoutEffect(() => {
    const el = row.current?.querySelector<HTMLElement>(`[data-tab="${value}"]`);
    if (el) setPill({ x: el.offsetLeft, w: el.offsetWidth });
  }, [value]);
  useLayoutEffect(() => {
    if (pill && !placed) requestAnimationFrame(() => setPlaced(true));
  }, [pill, placed]);
  return (
    <div ref={row} role="tablist" className="relative flex items-center gap-1">
      {pill && (
        <span
          aria-hidden
          className="absolute top-0 left-0 h-full rounded-md bg-white/8 shadow-[inset_0_1px_0_oklch(1_0_0/0.05)]"
          style={{
            width: pill.w,
            transform: `translateX(${pill.x}px)`,
            transition: placed ? "transform 380ms var(--ease-out-quint), width 380ms var(--ease-out-quint)" : "none",
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
          className={`relative rounded-md px-3 py-1.5 text-sm ${
            value === id ? "text-zinc-100" : "text-zinc-400 hover:text-zinc-200"
          }`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="mt-8">
      <h2 className="text-sm font-medium text-zinc-300">{title}</h2>
      <div className="mt-3 divide-y divide-white/6 surface rounded-xl">
        {children}
      </div>
    </section>
  );
}

/** One setting per row: label on the left, control on the right. */
export function Row({ label, children, htmlFor }: { label: string; children: ReactNode; htmlFor?: string }) {
  return (
    <div className="flex min-h-12 items-center justify-between gap-4 px-4 py-2.5 transition-colors duration-200 hover:bg-white/[0.025]">
      <label htmlFor={htmlFor} className="text-sm text-zinc-100">
        {label}
      </label>
      <div className="flex shrink-0 items-center gap-2">{children}</div>
    </div>
  );
}

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
      className={`group/toggle relative h-6 w-10 rounded-full shadow-[inset_0_1px_2px_oklch(0_0_0/0.35)] disabled:opacity-40 ${
        checked ? "bg-accent" : "bg-zinc-700"
      }`}
    >
      <span
        className={`absolute top-0.5 left-0.5 h-5 w-5 rounded-full shadow-[0_1px_2px_oklch(0_0_0/0.4)] transition-[transform,background-color,width] duration-300 ease-[var(--ease-out-quint)] group-active/toggle:w-6 ${
          checked ? "translate-x-4 bg-accent-ink group-active/toggle:translate-x-3" : "bg-zinc-200"
        }`}
      />
    </button>
  );
}

export const selectClass =
  "max-w-64 rounded-md border border-white/10 bg-zinc-900 px-2.5 py-1.5 text-sm text-zinc-100 focus:border-zinc-400 focus:outline-none disabled:opacity-40";

export const buttonClass =
  "rounded-md border border-white/10 px-3 py-1.5 text-sm text-zinc-200 hover:bg-white/5 disabled:opacity-40";

export const primaryClass =
  "rounded-md bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 shadow-[0_1px_2px_oklch(0_0_0/0.3)] hover:bg-white disabled:opacity-40";

export const inputClass =
  "min-w-0 rounded-md border border-white/10 bg-zinc-900 px-2.5 py-1.5 text-sm text-zinc-100 placeholder:text-zinc-500 focus:border-zinc-400 focus:outline-none";

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
        className="w-40 accent-[var(--color-accent)] disabled:opacity-40"
      />
      <span className="w-12 text-right text-sm text-zinc-400 tabular-nums">{format(value)}</span>
    </>
  );
}

/** Key caps for a hotkey. */
export function Keys({ names, label }: { names: string[]; label: (n: string) => string }) {
  return (
    <span className="flex gap-1">
      {names.map((n) => (
        <kbd
          key={n}
          className="rounded border border-white/15 bg-zinc-800 px-1.5 py-0.5 font-sans text-xs text-zinc-200"
        >
          {label(n)}
        </kbd>
      ))}
    </span>
  );
}
