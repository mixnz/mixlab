import { describe, expect, it } from "vitest";
import { detectFormat } from "./detect";

describe("detectFormat", () => {
  it("guesses by the first non-whitespace character", () => {
    expect(detectFormat('  {"a":1}')).toBe("json");
    expect(detectFormat("[1,2]")).toBe("json");
    expect(detectFormat("\n<a/>")).toBe("xml");
    expect(detectFormat("SELECT 1")).toBe("sql");
  });

  it("returns null when there is nothing to guess from", () => {
    expect(detectFormat("   \n ")).toBeNull();
  });
});
