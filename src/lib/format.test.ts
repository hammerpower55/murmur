import { describe, expect, it } from "vitest";
import {
  appLabel,
  dateline,
  dayLabel,
  dayOfMonth,
  formatBytes,
  formatDelta,
  formatDuration,
  formatMinutes,
  formatRam,
  formatTime,
  formatWhen,
  groupByDay,
  percent,
  plural,
  posterNumber,
  shortDate,
  supportsLanguage,
} from "./format";

describe("days", () => {
  const now = new Date(2026, 10, 9, 16, 0);

  it("names recent days and adds the year only from another year", () => {
    expect(dayLabel("2026-11-09T01:00:00", now)).toBe("Today");
    expect(dayLabel("2026-11-08T23:59:00", now)).toBe("Yesterday");
    expect(dayLabel("2026-10-28T12:00:00", now)).toBe("28 Oct");
    expect(dayLabel("2025-10-28T12:00:00", now)).toBe("28 Oct 2025");
  });

  it("groups rows by day and keeps their order", () => {
    const rows = ["2026-11-09T14:00:00", "2026-11-09T09:00:00", "2026-11-08T20:00:00", "2026-10-28T08:00:00"].map(
      (createdAt) => ({ createdAt }),
    );
    expect(groupByDay(rows, now).map((g) => [g.label, g.rows.length])).toEqual([
      ["Today", 2],
      ["Yesterday", 1],
      ["28 Oct", 1],
    ]);
    expect(groupByDay([], now)).toEqual([]);
  });

  it("writes the time and the dateline", () => {
    expect(formatTime("2026-11-09T14:05:00")).toBe("2:05 PM");
    expect(dateline(now)).toBe("Monday 9 November");
    expect(dayOfMonth("2026-10-28T12:00:00")).toBe(28);
  });
});

describe("posterNumber", () => {
  it("opens up the commas and leaves short numbers alone", () => {
    expect(posterNumber(12480)).toBe("12, 480");
    expect(posterNumber(1234567)).toBe("1, 234, 567");
    expect(posterNumber(480)).toBe("480");
  });
});

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
    expect(appLabel("Slack.app")).toBe("Slack");
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
