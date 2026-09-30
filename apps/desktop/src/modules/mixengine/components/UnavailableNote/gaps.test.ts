import { describe, expect, it } from "vitest";
import { describeGaps } from "./gaps";

describe("describeGaps", () => {
  it("names every kind that could not be read, and gives each reason for the tooltip", () => {
    const said = describeGaps([
      { name: "node", reason: "does not hash" },
      { name: "go", reason: "is not published" },
    ]);
    expect(said).toEqual({ names: "node, go", title: "node: does not hash\ngo: is not published" });
  });

  it("says nothing when everything was read, or when the daemon is older than the field", () => {
    expect(describeGaps([])).toBeNull();
    expect(describeGaps(undefined)).toBeNull();
    expect(describeGaps(null)).toBeNull();
  });
});
