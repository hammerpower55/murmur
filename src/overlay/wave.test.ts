import { describe, expect, it } from "vitest";
import {
  ATTACK,
  BREATH,
  FLOW_REST,
  FLOW_VOICE,
  RELEASE,
  SWELL,
  THINK_RADIUS,
  Wave,
  approach,
  flowRate,
  palette,
  shape,
  targetScale,
} from "./wave";

describe("Wave", () => {
  it("stays at rest in silence", () => {
    const w = new Wave();
    w.push(0, 0);
    w.push(0, 50);
    expect(w.valueAt(100)).toBe(0);
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

  it("never leaves 0 to 1, however loud", () => {
    const w = new Wave();
    for (let t = 0; t < 2000; t += 50) {
      w.push(5, t);
      expect(w.valueAt(t + 25)).toBeLessThanOrEqual(1);
      expect(w.valueAt(t + 25)).toBeGreaterThanOrEqual(0);
    }
  });

  it("reset clears the voice", () => {
    const w = new Wave();
    w.push(1, 0);
    w.push(1, 50);
    w.reset(100);
    expect(w.valueAt(100)).toBe(0);
  });
});

describe("shape", () => {
  it("leaves room noise at rest and lifts quiet speech", () => {
    expect(shape(0.05)).toBe(0);
    expect(shape(0.25)).toBeGreaterThan(0.35);
    expect(shape(0.5)).toBeGreaterThan(shape(0.25));
    expect(shape(1)).toBe(1);
    expect(shape(3)).toBe(1);
  });
});

describe("the orb's size", () => {
  it("breathes gently at rest, and not at all under reduced motion", () => {
    for (let t = 0; t < 5000; t += 37) {
      expect(Math.abs(targetScale("recording", 0, t) - 1)).toBeLessThanOrEqual(BREATH + 1e-9);
      expect(targetScale("recording", 0, t, true)).toBe(1);
    }
  });

  it("swells with the voice", () => {
    expect(targetScale("recording", 0.5, 0, true)).toBeGreaterThan(targetScale("recording", 0.2, 0, true));
    expect(targetScale("recording", 1, 0)).toBeCloseTo(1 + SWELL);
  });

  it("still follows the voice under reduced motion", () => {
    expect(targetScale("recording", 1, 0, true)).toBeCloseTo(1 + SWELL);
  });

  it("is smaller while transcribing, and holds still under reduced motion", () => {
    for (let t = 0; t < 3000; t += 41) {
      expect(targetScale("processing", 0, t)).toBeLessThan(1);
    }
    expect(targetScale("processing", 0, 300, true)).toBe(THINK_RADIUS);
    expect(targetScale("done", 0, 0)).toBe(1);
    expect(targetScale("cancelled", 0, 0)).toBeLessThan(1);
  });
});

describe("the orb's motion", () => {
  it("stirs faster with the voice", () => {
    expect(flowRate("recording", 0)).toBe(FLOW_REST);
    expect(flowRate("recording", 1)).toBe(FLOW_VOICE);
    expect(flowRate("processing", 0)).toBeGreaterThan(FLOW_REST);
  });

  it("glides to a target the same way at any frame rate", () => {
    expect(approach(0, 1, 0, 100)).toBe(0);
    expect(approach(0, 1, 100000, 100)).toBeCloseTo(1);
    const once = approach(0, 1, 32, 100);
    const twice = approach(approach(0, 1, 16, 100), 1, 16, 100);
    expect(once).toBeCloseTo(twice);
  });
});

describe("palette", () => {
  const luma = ([r, g, b]: number[]) => 0.3 * r + 0.59 * g + 0.11 * b;

  it("keeps the chosen colour as it is", () => {
    const [r, g, b] = palette("#f6b83d").base;
    expect(r * 255).toBeCloseTo(0xf6, 0);
    expect(g * 255).toBeCloseTo(0xb8, 0);
    expect(b * 255).toBeCloseTo(0x3d, 0);
  });

  it("runs from a deep rose up to a pale highlight", () => {
    const p = palette("#f6b83d");
    expect(luma(p.rose)).toBeLessThan(luma(p.coral));
    expect(luma(p.coral)).toBeLessThan(luma(p.base));
    expect(luma(p.base)).toBeLessThan(luma(p.gold));
    expect(luma(p.gold)).toBeLessThan(luma(p.pale));
  });

  it("gives amber a warm coral, not green", () => {
    const [r, g] = palette("#f6b83d").coral;
    expect(r).toBeGreaterThan(g);
  });

  it("works for any colour without leaving range", () => {
    for (const hex of ["#000000", "#ffffff", "#3366ff", "#22cc88", "#ff0000"]) {
      for (const rgb of Object.values(palette(hex))) {
        for (const c of rgb) {
          expect(Number.isFinite(c)).toBe(true);
          expect(c).toBeGreaterThanOrEqual(0);
          expect(c).toBeLessThanOrEqual(1);
        }
      }
    }
  });
});
