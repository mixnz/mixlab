import { describe, expect, it } from "vitest";
import { runRegex } from "./match";

const texts = (pattern: string, flags: string, subject: string): string[] => {
  const result = runRegex(pattern, flags, subject, "");
  return result.ok ? result.matches.map((match) => match.text) : [`LỖI:${result.message}`];
};

describe("runRegex", () => {
  it("thu mọi match với cờ g", () => {
    expect(texts("\\d+", "g", "a1b22c333")).toEqual(["1", "22", "333"]);
  });

  it("chỉ thu match đầu khi không có cờ g", () => {
    expect(texts("\\d+", "", "a1b22")).toEqual(["1"]);
  });

  // An empty-matching pattern leaves `lastIndex` in place and `exec` returns forever. The loop has
  // to push it forward itself.
  it("không treo với mẫu khớp rỗng", () => {
    expect(texts("(?=a)", "g", "aaa")).toEqual(["", "", ""]);
    expect(runRegex("a*", "g", "bb", "").ok).toBe(true);
  });

  it("trả vị trí của từng match", () => {
    const result = runRegex("b", "g", "abcb", "");
    expect(result.ok && result.matches.map((match) => match.index)).toEqual([1, 3]);
  });

  it("liệt kê nhóm bắt theo số", () => {
    const result = runRegex("(\\w)(\\d)", "", "a1", "");
    expect(result.ok && result.matches[0]?.groups).toEqual([
      { name: null, index: 1, text: "a" },
      { name: null, index: 2, text: "1" },
    ]);
  });

  it("liệt kê nhóm có tên", () => {
    const result = runRegex("(?<chu>\\w)", "", "a", "");
    expect(result.ok && result.matches[0]?.groups).toContainEqual({
      name: "chu",
      index: -1,
      text: "a",
    });
  });

  it("để null cho nhóm không khớp", () => {
    const result = runRegex("(a)|(b)", "", "a", "");
    expect(result.ok && result.matches[0]?.groups[1]?.text).toBeNull();
  });

  it("in bản xem trước sau khi thay thế", () => {
    const result = runRegex("(\\d)", "g", "a1b2", "[$1]");
    expect(result.ok && result.replaced).toBe("a[1]b[2]");
  });

  it("trả nguyên văn thông báo của engine khi mẫu sai", () => {
    const result = runRegex("(", "", "a", "");
    expect(result.ok).toBe(false);
  });

  it("cắt bớt khi có quá nhiều match", () => {
    const result = runRegex("a", "g", "a".repeat(600), "");
    expect(result.ok && result.truncated).toBe(true);
    expect(result.ok && result.matches.length).toBe(500);
  });
});
