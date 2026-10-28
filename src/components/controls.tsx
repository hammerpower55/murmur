// Small form controls shared by Settings, Dictionary and Snippets.

import type { ReactNode } from "react";

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="mt-8">
      <h2 className="text-sm font-medium text-zinc-300">{title}</h2>
      <div className="mt-3 divide-y divide-white/6 rounded-xl border border-white/8 bg-zinc-900/60">
        {children}
      </div>
    </section>
  );
}

/** One setting per row: label on the left, control on the right. */
export function Row({ label, children, htmlFor }: { label: string; children: ReactNode; htmlFor?: string }) {
  return (
    <div className="flex min-h-12 items-center justify-between gap-4 px-4 py-2.5">
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
      className={`relative h-6 w-10 rounded-full transition-colors disabled:opacity-40 ${
        checked ? "bg-zinc-100" : "bg-zinc-700"
      }`}
    >
      <span
        className={`absolute top-0.5 left-0.5 h-5 w-5 rounded-full transition-transform ${
          checked ? "translate-x-4 bg-zinc-900" : "bg-zinc-300"
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
  "rounded-md bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 hover:bg-white disabled:opacity-40";

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
        className="w-40 accent-zinc-100 disabled:opacity-40"
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
