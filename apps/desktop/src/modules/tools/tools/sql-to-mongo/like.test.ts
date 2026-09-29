import { describe, expect, it } from "vitest";
import { likeToRegex } from "./like";

describe("likeToRegex", () => {
  it("turns % into .* and _ into .", () => {
    expect(likeToRegex("a%b_c")).toBe("^a.*b.c$");
  });

  it("drops the start anchor when the pattern starts with %", () => {
    expect(likeToRegex("%abc")).toBe(".*abc$");
  });

  it("drops the end anchor when the pattern ends with %", () => {
    expect(likeToRegex("abc%")).toBe("^abc.*");
  });

  it("escapes regex special characters — this is where a silent bug is born", () => {
    expect(likeToRegex("a.b%")).toBe("^a\\.b.*");
    expect(likeToRegex("(x)%")).toBe("^\\(x\\).*");
    expect(likeToRegex("a+b")).toBe("^a\\+b$");
    expect(likeToRegex("100$")).toBe("^100\\$$");
  });

  it("a pattern of only % matches everything", () => {
    expect(likeToRegex("%")).toBe(".*");
  });

  it("a pattern without wildcards is exact equality, not contains", () => {
    expect(likeToRegex("abc")).toBe("^abc$");
  });
});
