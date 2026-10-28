import { describe, expect, it } from "vitest";
import { formatBytes, formatRam, percent, supportsLanguage } from "./format";

describe("formatBytes", () => {
  it("matches the sizes Windows shows", () => {
    expect(formatBytes(874188075)).toBe("834 MB");
    expect(formatBytes(652184014 + 8998286 + 139764 + 9384)).toBe("631 MB");
    expect(formatBytes(1624555275)).toBe("1.5 GB");
    expect(formatBytes(9384)).toBe("9 KB");
    expect(formatBytes(10)).toBe("1 KB");
  });
});

describe("formatRam", () => {
  it("uses GB from a thousand MB", () => {
    expect(formatRam(400)).toBe("400 MB RAM");
    expect(formatRam(1800)).toBe("1.8 GB RAM");
  });
});

describe("percent", () => {
  it("floors and clamps", () => {
    expect(percent(0, 100)).toBe(0);
    expect(percent(999, 1000)).toBe(99);
    expect(percent(1000, 1000)).toBe(100);
    expect(percent(5, 0)).toBe(0);
  });
});

describe("supportsLanguage", () => {
  it("treats * as every language and an empty filter as all", () => {
    expect(supportsLanguage(["*"], "ur")).toBe(true);
    expect(supportsLanguage(["en"], "ur")).toBe(false);
    expect(supportsLanguage(["en"], "")).toBe(true);
  });
});
