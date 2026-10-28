// Murmur's mark: a lowercase m and a full stop (docs/brand). The full stop
// breathes while Murmur is listening or transcribing, the way the tray
// icon's turns red.

import type { Activity } from "../store/dictation";

export function Mark({ activity = "idle", className = "" }: { activity?: Activity; className?: string }) {
  return (
    <svg viewBox="43 95 188 110" className={className} aria-hidden>
      <g fill="currentColor">
        <path d="M43 136 V190 A15 15 0 0 0 73 190 V136 Z" />
        <path d="M99 136 V190 A15 15 0 0 0 129 190 V136 Z" />
        <path d="M155 136 V190 A15 15 0 0 0 185 190 V136 Z" />
        <path d="M43 138 A43 43 0 0 1 129 138 L99 138 A13 13 0 0 0 73 138 Z" />
        <path d="M99 138 A43 43 0 0 1 185 138 L155 138 A13 13 0 0 0 129 138 Z" />
      </g>
      <circle
        cx="212"
        cy="186"
        r="19"
        className="fill-accent"
        style={activity === "idle" ? undefined : { animation: "breathe 1.2s var(--ease-in-out-quart) infinite" }}
      />
    </svg>
  );
}
