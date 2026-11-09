// Waveform maths for the overlay (spec 6.4). Kept free of the DOM so it can
// be unit tested.

export const BAR_COUNT = 13;
export const CENTRE = (BAR_COUNT - 1) / 2;
/** Bar height at silence, px: a row of dots. */
export const MIN_HEIGHT = 3;
/** Extra height at full level, px. */
export const RANGE = 15;
export const MAX_HEIGHT = MIN_HEIGHT + RANGE;
/** Each bar lags its inner neighbour by this much, so the wave ripples out. */
export const LAG_MS = 22;
/** Exponential smoothing per level event: quick to rise, slower to fall. */
export const ATTACK = 0.55;
export const RELEASE = 0.3;
/** Level events arrive this often. */
export const EVENT_MS = 50;
/** How much the outer bars are shortened (0 = flat row, 1 = outer bars vanish). */
export const TAPER = 0.55;
/** How far each bar wanders around its share of the level, 0 to 1. */
export const WOBBLE = 0.22;

/** Levels at or below this (room noise) leave the wave at rest. */
export const GATE = 0.06;
/** Below 1 lifts quiet speech so it still moves a 32 px pill. */
export const CURVE = 0.55;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Maps a mic level (0 to 1, from -50 to -10 dBFS) onto how far the wave moves. */
export function shape(level: number): number {
  const v = clamp((level - GATE) / (1 - GATE), 0, 1);
  return Math.pow(v, CURVE);
}

/** 1 at the centre bar, falling towards the edges so the wave is a soft hill. */
export function taper(i: number): number {
  const d = Math.abs(i - CENTRE) / (CENTRE + 1);
  return 1 - TAPER * d * d;
}

/**
 * A slow per bar sway so a steady voice does not look like a frozen bar
 * chart. Only scales the voiced part, so silence stays flat.
 */
export function wobble(i: number, now: number): number {
  return 1 - WOBBLE / 2 + (WOBBLE / 2) * Math.sin(now * 0.0071 + i * 1.83);
}

/**
 * A smoothed level that can be read at any moment. Each event moves the
 * smoothed target; frames in between interpolate towards it so bars glide
 * instead of stepping. A short history lets outer bars show the past.
 */
export class Wave {
  private from = 0;
  private to = 0;
  private at = 0;
  private history: { t: number; v: number }[] = [];

  reset(now: number) {
    this.from = 0;
    this.to = 0;
    this.at = now;
    this.history = [];
  }

  /** A level event, 0 to 1. */
  push(level: number, now: number) {
    const target = shape(level);
    const alpha = target > this.to ? ATTACK : RELEASE;
    this.from = this.valueAt(now);
    this.to = alpha * target + (1 - alpha) * this.to;
    this.at = now;
  }

  valueAt(now: number): number {
    const k = clamp((now - this.at) / EVENT_MS, 0, 1);
    return this.from + (this.to - this.from) * k;
  }

  /** Call once per frame. Returns the current value. */
  sample(now: number): number {
    const v = this.valueAt(now);
    this.history.push({ t: now, v });
    const keep = now - (CENTRE * LAG_MS + 100);
    while (this.history.length > 1 && this.history[0].t < keep) {
      this.history.shift();
    }
    return v;
  }

  private valueBefore(t: number): number {
    for (let i = this.history.length - 1; i >= 0; i--) {
      if (this.history[i].t <= t) return this.history[i].v;
    }
    return 0;
  }

  /**
   * Levels per bar, 0 to 1: centre bar newest, outer bars up to 132 ms
   * behind and shorter. With `still` set (reduced motion) every bar shows
   * the current level, tapered but without ripple or sway.
   */
  levels(now: number, still = false): number[] {
    return Array.from({ length: BAR_COUNT }, (_, i) => {
      if (still) return this.valueAt(now) * taper(i);
      const delay = Math.abs(i - CENTRE) * LAG_MS;
      return this.valueBefore(now - delay) * taper(i) * wobble(i, now);
    });
  }

  /** Heights in px. */
  heights(now: number, still = false): number[] {
    return this.levels(now, still).map((v) => MIN_HEIGHT + v * RANGE);
  }
}
