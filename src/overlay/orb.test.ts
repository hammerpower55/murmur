import { describe, expect, it } from "vitest";
import {
  CANVAS,
  CROSS,
  PITCH,
  REST_RADIUS,
  TICK,
  dotLight,
  gradientAt,
  gridDots,
  onGlyph,
  sweepLight,
} from "./orb";
import { CANCEL_RADIUS, SWELL, palette } from "./wave";

describe("the grid", () => {
  it("is centred on a dot, symmetrical and inside its radius", () => {
    const dots = gridDots(PITCH, REST_RADIUS);
    expect(dots).toContainEqual({ x: 0, y: 0 });
    for (const d of dots) {
      expect(Math.hypot(d.x, d.y)).toBeLessThanOrEqual(REST_RADIUS);
      expect(dots.some((o) => o.x === -d.x && o.y === -d.y)).toBe(true);
    }
  });

  it("has enough dots to read as a ball, and few enough to draw every frame", () => {
    const dots = gridDots(PITCH, CANVAS / 2).length;
    expect(gridDots(PITCH, REST_RADIUS).length).toBeGreaterThan(80);
    expect(dots).toBeLessThan(400);
  });

  it("fits the canvas at a full voice", () => {
    expect(REST_RADIUS * (1 + SWELL)).toBeLessThan(CANVAS / 2);
  });
});

describe("dotLight", () => {
  it("stays between 0 and 1", () => {
    for (let flow = 0; flow < 20; flow += 0.7) {
      for (const level of [0, 0.5, 1]) {
        for (const [x, y] of [[0, 0], [0.5, 0.5], [-0.9, 0.2], [1, 0], [0.8, 0.8]]) {
          const v = dotLight(x, y, flow, level);
          expect(v).toBeGreaterThanOrEqual(0);
          expect(v).toBeLessThanOrEqual(1);
        }
      }
    }
  });

  const average = (x: number, y: number, level: number) => {
    let sum = 0;
    for (let flow = 0; flow < 50; flow += 0.31) sum += dotLight(x, y, flow, level);
    return sum / Math.ceil(50 / 0.31);
  };

  it("is brighter in the middle than at the edge, which makes it read as round", () => {
    expect(average(0, 0, 0)).toBeGreaterThan(average(0.95, 0, 0));
  });

  it("is brighter with a voice", () => {
    expect(average(0.2, 0.2, 1)).toBeGreaterThan(average(0.2, 0.2, 0));
  });

  it("moves: a dot changes as the pattern flows", () => {
    expect(dotLight(0.3, 0.2, 0, 0)).not.toBeCloseTo(dotLight(0.3, 0.2, 2, 0), 2);
  });
});

describe("sweepLight", () => {
  it("is brightest at the head of the comet and fades behind it", () => {
    const head = 1;
    const at = (behind: number) => {
      const a = head * 3.2 - behind;
      return sweepLight(0.8 * Math.cos(a), 0.8 * Math.sin(a), head);
    };
    expect(at(0.01)).toBeGreaterThan(at(1));
    expect(at(1)).toBeGreaterThan(at(3));
  });
});

describe("the tick and cross", () => {
  const litDots = (glyph: typeof TICK) => gridDots(PITCH, REST_RADIUS).filter((d) => onGlyph(d.x, d.y, glyph, PITCH));

  it("are drawn in a handful of dots, inside the ball", () => {
    expect(litDots(TICK)).toHaveLength(7);
    expect(litDots(CROSS)).toHaveLength(9);
    // The tick shows on the full size ball, the cross on the smaller cancelled one.
    for (const [x, y] of TICK.flat()) expect(Math.hypot(x, y) * PITCH).toBeLessThan(REST_RADIUS);
    for (const [x, y] of CROSS.flat()) expect(Math.hypot(x, y) * PITCH).toBeLessThan(REST_RADIUS * CANCEL_RADIUS);
  });

  it("has a symmetrical cross, and a tick that is not", () => {
    const cross = litDots(CROSS);
    expect(cross.every((d) => cross.some((o) => o.x === -d.x && o.y === d.y))).toBe(true);
    const tick = litDots(TICK);
    expect(tick.every((d) => tick.some((o) => o.x === -d.x && o.y === d.y))).toBe(false);
  });
});

describe("gradientAt", () => {
  const p = palette("#f6b83d");

  it("runs from gold to rose through the chosen colour", () => {
    expect(gradientAt(p, 0)).toEqual(p.gold);
    expect(gradientAt(p, 1 / 3)[0]).toBeCloseTo(p.base[0]);
    expect(gradientAt(p, 1)[0]).toBeCloseTo(p.rose[0]);
  });

  it("stays in range outside 0 to 1", () => {
    expect(gradientAt(p, -1)).toEqual(p.gold);
    expect(gradientAt(p, 2)[2]).toBeCloseTo(p.rose[2]);
  });
});
