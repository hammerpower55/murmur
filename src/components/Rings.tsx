// Murmur's full stop grown to poster size, with sound leaving it in arcs. The
// arcs swing into place when a page opens, turn as the page scrolls, spin while
// Murmur listens or transcribes, and swell with the voice.

import { useEffect, useMemo, useRef } from "react";
import { events } from "../lib/api";
import { voiceLevel } from "../lib/motion";
import { ringsFor } from "../lib/rings";
import { useDictation } from "../store/dictation";

const STEP_MS = 70;

export function Rings({ seed, className = "" }: { seed: number; className?: string }) {
  const activity = useDictation((d) => d.activity);
  const rings = useMemo(() => ringsFor(seed), [seed]);
  const root = useRef<SVGSVGElement>(null);

  // Written straight to the style: this runs 20 times a second and need not
  // touch React.
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    if (activity !== "recording") {
      el.style.setProperty("--level", "0");
      return;
    }
    const off = events.audioLevel((rms) => el.style.setProperty("--level", voiceLevel(rms).toFixed(3)));
    return () => void off.then((f) => f());
  }, [activity]);

  return (
    <svg ref={root} viewBox="-100 -100 200 200" data-activity={activity} className={`rings ${className}`} aria-hidden>
      <g className="rings-scroll">
        <g className="rings-swell">
          {rings.map((ring, i) => (
            <g
              key={ring.r}
              className="ring"
              style={
                {
                  "--start": `${ring.start}deg`,
                  "--delay": `${i * STEP_MS}ms`,
                } as React.CSSProperties
              }
            >
              <circle
                className="ring-live"
                r={ring.r}
                fill="none"
                stroke={ring.hot ? "var(--accent)" : "currentColor"}
                strokeWidth={ring.width}
                pathLength={100}
                strokeDasharray={`${ring.arc} ${100 - ring.arc}`}
                style={
                  {
                    "--spin": `${Math.abs(ring.spin)}s`,
                    animationDirection: ring.spin < 0 ? "reverse" : "normal",
                  } as React.CSSProperties
                }
              />
            </g>
          ))}
        </g>
        <g className="rings-core-in">
          <circle className="rings-core" r={14} />
        </g>
      </g>
    </svg>
  );
}
