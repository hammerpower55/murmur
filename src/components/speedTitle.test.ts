import { describe, expect, it } from "vitest";
import type { ModelInfo } from "../lib/api";
import { speedTitle } from "./ModelCard";

const model = (clipMs: number, measured = false) => ({ clipMs, measured }) as ModelInfo;

describe("speedTitle", () => {
  it("names the chip and says when the time is an estimate", () => {
    expect(speedTitle(model(150), "Apple M2")).toBe("About 0.1 s for a 5 s clip on Apple M2, estimated");
    expect(speedTitle(model(4500, true), "Snapdragon X Plus")).toBe("About 4.5 s for a 5 s clip on Snapdragon X Plus");
    expect(speedTitle(model(12000), "Snapdragon X Plus")).toBe("About 12 s for a 5 s clip on Snapdragon X Plus, estimated");
    expect(speedTitle(model(2000))).toBe("About 2 s for a 5 s clip on this PC, estimated");
  });

  it("admits when the speed is not known", () => {
    expect(speedTitle(model(0), "Apple M1")).toBe("Speed on Apple M1 is not known");
  });
});
