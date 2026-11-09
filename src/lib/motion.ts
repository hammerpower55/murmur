// Motion helpers. Everything here falls back to an instant change when the
// user asks Windows for reduced motion.

import { useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";

export function prefersReducedMotion(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

/** Exponential ease out: fast start, long gentle landing. 0 to 1 in, 0 to 1 out. */
export function easeOutExpo(t: number): number {
  if (t <= 0) return 0;
  if (t >= 1) return 1;
  return 1 - 2 ** (-10 * t);
}

/** How far a voice moves the sound rings, 0 to 1. Speech is quiet in RMS terms,
 * so the square root spreads it out. */
export function voiceLevel(rms: number): number {
  if (!Number.isFinite(rms) || rms <= 0) return 0;
  return Math.min(1, Math.sqrt(rms) * 1.6);
}

/** Runs a change of page inside a view transition where there is one (macOS 14's
 * WebKit has none, so there it is simply instant). */
export function withViewTransition(update: () => void) {
  if (prefersReducedMotion() || !document.startViewTransition) {
    update();
    return;
  }
  document.startViewTransition(() => flushSync(update));
}

/** Changes the theme with the new one spreading out from `origin`. Rust sets the
 * window's theme, so this waits for the page to hear of it, at most `waitMs`. */
export function themeWipe(change: () => void | Promise<unknown>, origin: { x: number; y: number }, waitMs = 300) {
  if (prefersReducedMotion() || !document.startViewTransition) {
    void change();
    return;
  }
  const root = document.documentElement;
  root.style.setProperty("--wipe-x", `${origin.x}px`);
  root.style.setProperty("--wipe-y", `${origin.y}px`);
  root.classList.add("theme-wipe");
  const scheme = window.matchMedia("(prefers-color-scheme: dark)");
  const transition = document.startViewTransition(async () => {
    const heard = new Promise<void>((resolve) => {
      scheme.addEventListener("change", () => resolve(), { once: true });
      window.setTimeout(resolve, waitMs);
    });
    await change();
    await heard;
  });
  void transition.finished.finally(() => root.classList.remove("theme-wipe"));
}

/** Counts from the last value shown (0 on first mount) up to `target`. */
export function useCountUp(target: number, durationMs = 900): number {
  const reduced = prefersReducedMotion();
  const [value, setValue] = useState(reduced ? target : 0);
  const shown = useRef(reduced ? target : 0);

  useEffect(() => {
    if (prefersReducedMotion() || !Number.isFinite(target)) {
      shown.current = target;
      setValue(target);
      return;
    }
    const from = shown.current;
    const start = performance.now();
    let frame = 0;
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / durationMs);
      const v = from + (target - from) * easeOutExpo(t);
      shown.current = v;
      setValue(v);
      if (t < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [target, durationMs]);

  return value;
}
