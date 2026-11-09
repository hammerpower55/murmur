import { describe, expect, it } from "vitest";
import {
  ATTACK,
  BAR_COUNT,
  CENTRE,
  LAG_MS,
  MAX_HEIGHT,
  MIN_HEIGHT,
  RELEASE,
  Wave,
  shape,
  taper,
  wobble,
} from "./wave";

/** Feeds a level event every 50 ms and samples every 10 ms. */
function run(wave: Wave, levels: number[], start = 0): number {
  let t = start;
  for (const level of levels) {
    wave.push(level, t);
    for (let f = 0; f < 5; f++) {
      wave.sample(t);
      t += 10;
    }
  }
  return t;
}

describe("Wave", () => {
  it("rests at the minimum height in silence", () => {
    const w = new Wave();
    const t = run(w, [0, 0, 0]);
    expect(w.heights(t)).toEqual(Array(BAR_COUNT).fill(MIN_HEIGHT));
    expect(w.heights(t, true)).toEqual(Array(BAR_COUNT).fill(MIN_HEIGHT));
  });

  it("rises quickly and falls more slowly", () => {
    const w = new Wave();
    w.push(1, 0);
    expect(w.valueAt(50)).toBeCloseTo(ATTACK);
    w.push(0, 50);
    expect(w.valueAt(100)).toBeCloseTo(ATTACK * (1 - RELEASE));
  });

  it("glides between events instead of jumping", () => {
    const w = new Wave();
    w.push(1, 0);
    expect(w.valueAt(0)).toBe(0);
    expect(w.valueAt(25)).toBeCloseTo(ATTACK / 2);
    expect(w.valueAt(50)).toBeCloseTo(ATTACK);
  });

  it("never exceeds the maximum height", () => {
    const w = new Wave();
    const t = run(w, Array(40).fill(5));
    for (let dt = 0; dt < 2000; dt += 7) {
      for (const h of w.heights(t + dt)) {
        expect(h).toBeLessThanOrEqual(MAX_HEIGHT + 1e-9);
        expect(h).toBeGreaterThanOrEqual(MIN_HEIGHT);
      }
    }
  });

  it("is a soft hill: the centre is tallest and the edges are shorter", () => {
    expect(taper(CENTRE)).toBe(1);
    expect(taper(0)).toBeLessThan(taper(3));
    expect(taper(0)).toBeCloseTo(taper(BAR_COUNT - 1));
    expect(taper(0)).toBeGreaterThan(0.4);
  });

  it("sways each bar by at most the wobble", () => {
    for (let i = 0; i < BAR_COUNT; i++) {
      for (let t = 0; t < 3000; t += 13) {
        const k = wobble(i, t);
        expect(k).toBeLessThanOrEqual(1);
        expect(k).toBeGreaterThanOrEqual(0.77);
      }
    }
  });

  it("ripples outward: the centre bar leads, outer bars lag", () => {
    const w = new Wave();
    const t = run(w, [0, 0, 1, 1, 1]);
    const v = w.levels(t);
    // Outer bars still show the silence from before the voice started.
    expect(v[CENTRE]).toBeGreaterThan(0.5);
    expect(v[0]).toBeLessThan(v[CENTRE] / 2);
  });

  it("outer bars show the centre value from earlier", () => {
    const w = new Wave();
    const t = run(w, [1, 1, 1, 1, 1, 0, 0, 0]);
    // The voice stopped: the edges still hold some of it, the centre has
    // mostly let go.
    const then = w.valueAt(t - CENTRE * LAG_MS);
    expect(then).toBeGreaterThan(w.valueAt(t));
    expect(w.levels(t, true)[0]).toBeLessThan(w.levels(t)[0]);
  });

  it("holds still under reduced motion", () => {
    const w = new Wave();
    const t = run(w, [1, 1, 1, 1]);
    const a = w.levels(t, true);
    const b = w.levels(t, true);
    expect(a).toEqual(b);
    // Symmetric, tapered, and the centre shows the current level.
    expect(a[CENTRE]).toBeCloseTo(w.valueAt(t));
    expect(a[0]).toBeCloseTo(a[BAR_COUNT - 1]);
  });

  it("leaves room noise at rest and lifts quiet speech", () => {
    expect(shape(0.05)).toBe(0);
    expect(shape(0.25)).toBeGreaterThan(0.35);
    expect(shape(0.5)).toBeGreaterThan(shape(0.25));
    expect(shape(1)).toBe(1);
    expect(shape(3)).toBe(1);
  });

  it("reset clears the history", () => {
    const w = new Wave();
    const t = run(w, [1, 1, 1]);
    w.reset(t);
    expect(w.heights(t)).toEqual(Array(BAR_COUNT).fill(MIN_HEIGHT));
  });
});
