import { describe, expect, it } from "vitest";
import { ringsFor } from "./rings";

describe("ringsFor", () => {
  it("draws the same picture for the same seed and another for another", () => {
    expect(ringsFor(3)).toEqual(ringsFor(3));
    expect(ringsFor(3)).not.toEqual(ringsFor(4));
  });

  it("stays inside the drawing with rings that never overlap", () => {
    for (let seed = 0; seed < 50; seed++) {
      const rings = ringsFor(seed);
      rings.forEach((ring, i) => {
        expect(ring.arc).toBeGreaterThanOrEqual(26);
        expect(ring.arc).toBeLessThanOrEqual(74);
        expect(ring.start).toBeGreaterThanOrEqual(0);
        expect(ring.start).toBeLessThan(360);
        expect(Math.abs(ring.spin)).toBeGreaterThanOrEqual(7);
        expect(ring.r + ring.width / 2).toBeLessThanOrEqual(100);
        if (i > 0) {
          const inner = rings[i - 1];
          expect(ring.r - ring.width / 2).toBeGreaterThan(inner.r + inner.width / 2);
        }
      });
      expect(rings.filter((r) => r.hot)).toHaveLength(1);
    }
  });
});
