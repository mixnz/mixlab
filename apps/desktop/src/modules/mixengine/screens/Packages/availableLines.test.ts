import { describe, expect, it } from "vitest";

import { groupByLine } from "./availableLines";

interface Row {
  kind: string;
  version: string;
  line?: string | null;
  newest_in_line?: boolean | null;
}

const row = (version: string, line: string | null, newest: boolean | null): Row => ({
  kind: "php",
  version,
  line,
  newest_in_line: newest,
});

const all = () => true;
const none = new Set<string>();

describe("groupByLine", () => {
  it("puts each line under its newest release", () => {
    const groups = groupByLine(
      [row("8.4.25", "8.4", true), row("8.4.24", "8.4", false), row("8.3.30", "8.3", true)],
      (r) => r.kind,
      all,
      none,
    );

    expect(groups.map((g) => g.head.version)).toEqual(["8.4.25", "8.3.30"]);
    expect(groups[0].others.map((r) => r.version)).toEqual(["8.4.24"]);
    expect(groups[0].open).toBe(false);
  });

  it("opens a line whose only match is an older release", () => {
    const groups = groupByLine(
      [row("8.4.25", "8.4", true), row("8.4.24", "8.4", false)],
      (r) => r.kind,
      (r) => r.version.startsWith("8.4.2") && r.version !== "8.4.25",
      none,
    );

    expect(groups).toHaveLength(1);
    expect(groups[0].open).toBe(true);
    expect(groups[0].others.map((r) => r.version)).toEqual(["8.4.24"]);
  });

  it("drops a line nothing in it matches", () => {
    const groups = groupByLine(
      [row("8.4.25", "8.4", true), row("8.3.30", "8.3", true)],
      (r) => r.kind,
      (r) => r.version === "8.3.30",
      none,
    );
    expect(groups.map((g) => g.key)).toEqual(["php@8.3"]);
  });

  it("keeps a line the person opened", () => {
    const groups = groupByLine(
      [row("8.4.25", "8.4", true), row("8.4.24", "8.4", false)],
      (r) => r.kind,
      all,
      new Set(["php@8.4"]),
    );
    expect(groups[0].open).toBe(true);
  });

  // Review focus 1: a daemon from before T193 sends no line.
  it("rows without a line are each their own group", () => {
    const groups = groupByLine([row("8.4.25", null, null), row("8.4.24", null, null)], (r) => r.kind, all, none);
    expect(groups.map((g) => g.head.version)).toEqual(["8.4.25", "8.4.24"]);
    expect(groups.every((g) => g.others.length === 0)).toBe(true);
  });
});
