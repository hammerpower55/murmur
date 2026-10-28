// Waveform maths for the overlay (spec 6.4). Kept free of the DOM so it can
// be unit tested.

export const BAR_COUNT = 9;
export const CENTRE = (BAR_COUNT - 1) / 2;
/** Bar height at silence, px. */
export const MIN_HEIGHT = 6;
/** Extra height at full level, px. */
export const RANGE = 34;
export const MAX_HEIGHT = MIN_HEIGHT + RANGE;
/** Each bar lags its inner neighbour by this much, so the wave ripples out. */
export const LAG_MS = 30;
/** Exponential smoothing per level event. */
export const ALPHA = 0.35;
/** Level events arrive this often. */
export const EVENT_MS = 50;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

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
    this.from = this.valueAt(now);
    this.to = ALPHA * clamp(level, 0, 1) + (1 - ALPHA) * this.to;
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

  /** Heights in px, centre bar newest, outer bars up to 120 ms behind. */
  heights(now: number): number[] {
    return Array.from({ length: BAR_COUNT }, (_, i) => {
      const delay = Math.abs(i - CENTRE) * LAG_MS;
      return MIN_HEIGHT + this.valueBefore(now - delay) * RANGE;
    });
  }
}
