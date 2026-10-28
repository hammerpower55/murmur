import { describe, expect, it } from "vitest";
import { activityOf } from "../store/dictation";
import { easeOutExpo } from "./motion";

describe("easeOutExpo", () => {
  it("starts at 0, lands exactly on 1, and front loads the movement", () => {
    expect(easeOutExpo(0)).toBe(0);
    expect(easeOutExpo(1)).toBe(1);
    expect(easeOutExpo(-1)).toBe(0);
    expect(easeOutExpo(2)).toBe(1);
    expect(easeOutExpo(0.3)).toBeGreaterThan(0.8);
  });

  it("never goes backwards", () => {
    let last = 0;
    for (let t = 0; t <= 1; t += 0.05) {
      const v = easeOutExpo(t);
      expect(v).toBeGreaterThanOrEqual(last);
      last = v;
    }
  });
});

describe("activityOf", () => {
  it("maps dictation states to what the wave shows", () => {
    expect(activityOf("recording")).toBe("recording");
    expect(activityOf("processing")).toBe("processing");
    for (const s of ["idle", "cancelled", "error"]) expect(activityOf(s)).toBe("idle");
  });
});
