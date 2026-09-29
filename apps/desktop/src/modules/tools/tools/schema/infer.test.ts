import { describe, expect, it } from "vitest";
import { inferSchema } from "./infer";

describe("inferSchema", () => {
  it("reads a single object", () => {
    expect(inferSchema({ id: 1, name: "An" })).toEqual([
      { name: "id", types: ["integer"], optional: false, isoLike: false },
      { name: "name", types: ["string"], optional: false, isoLike: false },
    ]);
  });

  it("merges the keys of every element in the sample array", () => {
    expect(inferSchema([{ a: 1 }, { b: "x" }])).toEqual([
      { name: "a", types: ["integer"], optional: true, isoLike: false },
      { name: "b", types: ["string"], optional: true, isoLike: false },
    ]);
  });

  it("a key present in every element is not optional", () => {
    const fields = inferSchema([{ a: 1 }, { a: 2 }]);
    expect(fields?.[0]?.optional).toBe(false);
  });

  // `integer` meeting `number` widens to `number`; both are not kept.
  it("widens integer to number when both are seen", () => {
    expect(inferSchema([{ a: 1 }, { a: 1.5 }])?.[0]?.types).toEqual(["number"]);
  });

  it("keeps null alongside the real type instead of swallowing it", () => {
    expect(inferSchema([{ a: 1 }, { a: null }])?.[0]?.types).toEqual(["integer", "null"]);
  });

  it("marks strings that look like ISO 8601", () => {
    const fields = inferSchema([{ at: "2026-08-28T00:00:00Z" }, { at: "2026-08-29T10:30:00Z" }]);
    expect(fields?.[0]?.isoLike).toBe(true);
  });

  it("does not mark them when one value is not ISO", () => {
    const fields = inferSchema([{ at: "2026-08-28T00:00:00Z" }, { at: "hôm qua" }]);
    expect(fields?.[0]?.isoLike).toBe(false);
  });

  it("descends into nested objects", () => {
    const fields = inferSchema({ user: { id: 1 } });
    expect(fields?.[0]?.types).toEqual(["object"]);
    expect(fields?.[0]?.children).toEqual([
      { name: "id", types: ["integer"], optional: false, isoLike: false },
    ]);
  });

  it("takes the element shape of an array of objects", () => {
    const fields = inferSchema({ tags: [{ n: "a" }, { n: "b" }] });
    expect(fields?.[0]?.types).toEqual(["array"]);
    expect(fields?.[0]?.children?.[0]?.name).toBe("n");
  });

  it("gives an empty array no children", () => {
    expect(inferSchema({ tags: [] })?.[0]?.children).toBeUndefined();
  });

  it("returns null when the input is neither an object nor an array of objects", () => {
    expect(inferSchema(42)).toBeNull();
    expect(inferSchema([1, 2])).toBeNull();
    expect(inferSchema(null)).toBeNull();
  });
});
