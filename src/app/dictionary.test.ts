import { describe, expect, it, vi } from "vitest";

// The page imports Tauri's event API; only the pure helper is under test.
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const { keepLearned } = await import("./Dictionary");

describe("keepLearned", () => {
  const before = [
    { spoken: "graph fauna", written: "Grafana", learned: true },
    { spoken: "clari tree", written: "ClariTree" },
  ];

  it("keeps the mark on learned entries the user left alone", () => {
    const pairs: [string, string][] = [
      ["graph fauna", "Grafana"],
      ["clari tree", "ClariTree"],
    ];
    expect(keepLearned(pairs, before)).toEqual([
      { spoken: "graph fauna", written: "Grafana", learned: true },
      { spoken: "clari tree", written: "ClariTree" },
    ]);
  });

  it("drops the mark once the user edits the entry", () => {
    expect(keepLearned([["graph fauna", "Grafana Cloud"]], before)).toEqual([
      { spoken: "graph fauna", written: "Grafana Cloud" },
    ]);
  });
});
