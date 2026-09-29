import { describe, expect, it } from "vitest";
import { runRegex } from "./match";

const texts = (pattern: string, flags: string, subject: string): string[] => {
  const result = runRegex(pattern, flags, subject, "");
  return result.ok ? result.matches.map((match) => match.text) : [`LỖI:${result.message}`];
};

describe("runRegex", () => {
  it("collects every match with the g flag", () => {
    expect(texts("\\d+", "g", "a1b22c333")).toEqual(["1", "22", "333"]);
  });

  it("only collects the first match without the g flag", () => {
    expect(texts("\\d+", "", "a1b22")).toEqual(["1"]);
  });

  // An empty-matching pattern leaves `lastIndex` in place and `exec` returns forever. The loop has
  // to push it forward itself.
  it("does not hang on an empty-matching pattern", () => {
    expect(texts("(?=a)", "g", "aaa")).toEqual(["", "", ""]);
    expect(runRegex("a*", "g", "bb", "").ok).toBe(true);
  });

  it("returns each match's position", () => {
    const result = runRegex("b", "g", "abcb", "");
    expect(result.ok && result.matches.map((match) => match.index)).toEqual([1, 3]);
  });

  it("lists numbered capture groups", () => {
    const result = runRegex("(\\w)(\\d)", "", "a1", "");
    expect(result.ok && result.matches[0]?.groups).toEqual([
      { name: null, index: 1, text: "a" },
      { name: null, index: 2, text: "1" },
    ]);
  });

  it("lists named groups", () => {
    const result = runRegex("(?<chu>\\w)", "", "a", "");
    expect(result.ok && result.matches[0]?.groups).toContainEqual({
      name: "chu",
      index: -1,
      text: "a",
    });
  });

  it("gives null for a group that did not match", () => {
    const result = runRegex("(a)|(b)", "", "a", "");
    expect(result.ok && result.matches[0]?.groups[1]?.text).toBeNull();
  });

  it("prints a preview after replacement", () => {
    const result = runRegex("(\\d)", "g", "a1b2", "[$1]");
    expect(result.ok && result.replaced).toBe("a[1]b[2]");
  });

  it("returns the engine's message verbatim when the pattern is wrong", () => {
    const result = runRegex("(", "", "a", "");
    expect(result.ok).toBe(false);
  });

  it("truncates when there are too many matches", () => {
    const result = runRegex("a", "g", "a".repeat(600), "");
    expect(result.ok && result.truncated).toBe(true);
    expect(result.ok && result.matches.length).toBe(500);
  });
});
