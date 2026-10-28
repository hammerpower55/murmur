// Murmur's small wave: resting when idle, rippling out from the centre in
// amber while recording, breathing while it transcribes. It echoes the
// overlay pill so the window and the pill speak the same language.

import type { Activity } from "../store/dictation";

/** Bars from the centre outwards get later delays, so the wave ripples out. */
const RIPPLE_MS = [240, 120, 0, 120, 240];
const REST = [0.35, 0.55, 0.75, 0.55, 0.35];

export function Wave({ activity, size = "sm" }: { activity: Activity; size?: "sm" | "lg" }) {
  const bar = size === "lg" ? "h-10 w-1.5" : "h-3.5 w-[3px]";
  const gap = size === "lg" ? "gap-1.5" : "gap-[2px]";
  return (
    <span className={`inline-flex items-center ${gap}`} aria-hidden>
      {RIPPLE_MS.map((delay, i) => {
        const style =
          activity === "recording"
            ? { animation: `wave 900ms var(--ease-in-out-quart) ${delay}ms infinite` }
            : activity === "processing"
              ? { transform: "scaleY(0.5)", animation: `breathe 1s var(--ease-in-out-quart) ${i * 120}ms infinite` }
              : { transform: `scaleY(${REST[i]})` };
        return (
          <span
            key={i}
            style={style}
            className={`${bar} rounded-full transition-[background-color,transform] duration-300 ${
              activity === "idle" ? "bg-zinc-500" : "bg-accent"
            }`}
          />
        );
      })}
    </span>
  );
}
