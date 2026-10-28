import { describe, expect, it } from "vitest";
import { ALPHA, BAR_COUNT, LAG_MS, MAX_HEIGHT, MIN_HEIGHT, Wave } from "./wave";

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
  });

  it("smooths each event with alpha 0.35", () => {
    const w = new Wave();
    w.push(1, 0);
    expect(w.valueAt(50)).toBeCloseTo(ALPHA);
    w.push(1, 50);
    expect(w.valueAt(100)).toBeCloseTo(ALPHA + (1 - ALPHA) * ALPHA);
  });

  it("glides between events instead of jumping", () => {
    const w = new Wave();
    w.push(1, 0);
    expect(w.valueAt(0)).toBe(0);
    expect(w.valueAt(25)).toBeCloseTo(ALPHA / 2);
    expect(w.valueAt(50)).toBeCloseTo(ALPHA);
  });

  it("never exceeds the maximum height", () => {
    const w = new Wave();
    const t = run(w, Array(40).fill(5));
    for (const h of w.heights(t)) {
      expect(h).toBeLessThanOrEqual(MAX_HEIGHT + 1e-9);
    }
  });

  it("ripples outward: the centre bar leads, outer bars lag", () => {
    const w = new Wave();
    const t = run(w, [0, 0, 1, 1, 1, 1]);
    const h = w.heights(t);
    expect(h[4]).toBeGreaterThan(h[3]);
    expect(h[3]).toBeGreaterThan(h[0]);
    // Symmetric around the centre.
    expect(h[3]).toBeCloseTo(h[5]);
    expect(h[0]).toBeCloseTo(h[8]);
  });

  it("outer bar shows the centre value from 120 ms earlier", () => {
    const w = new Wave();
    const t = run(w, [1, 1, 1, 1, 0, 0, 0, 0]);
    const centreThen = MIN_HEIGHT + w.valueAt(t - 4 * LAG_MS);
    expect(w.heights(t)[0]).toBeGreaterThan(w.heights(t)[4]);
    expect(w.heights(t)[0]).toBeLessThanOrEqual(centreThen + 34);
  });

  it("reset clears the history", () => {
    const w = new Wave();
    const t = run(w, [1, 1, 1]);
    w.reset(t);
    expect(w.heights(t)).toEqual(Array(BAR_COUNT).fill(MIN_HEIGHT));
  });
});
