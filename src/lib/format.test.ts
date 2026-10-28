import { describe, expect, it } from "vitest";
import {
  appLabel,
  formatBytes,
  formatDelta,
  formatDuration,
  formatMinutes,
  formatRam,
  formatWhen,
  percent,
  plural,
  shortDate,
  supportsLanguage,
} from "./format";

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

describe("formatWhen", () => {
  const now = new Date(2026, 9, 28, 15, 0);
  it("shows only the time today", () => {
    expect(formatWhen("2026-10-28T14:05:00", now)).toBe("2:05 PM");
  });
  it("says yesterday", () => {
    expect(formatWhen("2026-10-27T09:30:00", now)).toBe("Yesterday, 9:30 AM");
  });
  it("shows the date further back", () => {
    expect(formatWhen("2026-10-20T18:00:00", now)).toBe("20 Oct, 6:00 PM");
  });
});

describe("formatDuration", () => {
  it("uses seconds under a minute", () => {
    expect(formatDuration(4200)).toBe("4.2 s");
    expect(formatDuration(65_000)).toBe("1 min 5 s");
    expect(formatDuration(120_000)).toBe("2 min");
  });
});

describe("appLabel and plural", () => {
  it("drops .exe and handles unknown apps", () => {
    expect(appLabel("Code.exe")).toBe("Code");
    expect(appLabel(null)).toBe("Unknown app");
  });
  it("pluralises with thousands separators", () => {
    expect(plural(1, "word", "words")).toBe("1 word");
    expect(plural(1234, "word", "words")).toBe("1,234 words");
  });
});

describe("insights formatting", () => {
  it("formats time saved", () => {
    expect(formatMinutes(20.67)).toBe("21 min");
    expect(formatMinutes(65)).toBe("1 h 5 min");
    expect(formatMinutes(180)).toBe("3 h");
    expect(formatMinutes(-4)).toBe("0 min");
  });
  it("formats the weekly change", () => {
    expect(formatDelta(95, 95)).toBe("Same as last week");
    expect(formatDelta(1200, 200)).toBe("+1,000 vs last week");
    expect(formatDelta(3, 10)).toBe("-7 vs last week");
  });
  it("shortens dates", () => {
    expect(shortDate("2026-10-28")).toBe("28 Oct");
  });
});
