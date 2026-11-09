// A small soft wave beside the sidebar's status while Murmur is busy.

import type { Activity } from "../store/dictation";

/** Bars from the centre outwards get later delays, so the wave ripples out. */
const RIPPLE_MS = [240, 120, 0, 120, 240];

export function Wave({ activity }: { activity: Activity }) {
  return (
    <span className="inline-flex h-3 items-center gap-[2px]" aria-hidden>
      {RIPPLE_MS.map((delay, i) => (
        <span
          key={i}
          className="h-3 w-[2px] rounded-full bg-ink"
          style={
            activity === "recording"
              ? { animation: `wave 900ms var(--ease-in-out-quart) ${delay}ms infinite` }
              : { transform: "scaleY(0.4)", animation: `breathe 1s var(--ease-in-out-quart) ${i * 120}ms infinite` }
          }
        />
      ))}
    </span>
  );
}
