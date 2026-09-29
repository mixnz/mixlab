import { describe, expect, it } from "vitest";
import { buildSplitRows, computeLineSegments, diffLines, diffSegments, type DiffOptions } from "./diff";

const plain: DiffOptions = { ignoreWhitespace: false, ignoreCase: false };

const kinds = (left: string, right: string, options: DiffOptions = plain): string[] => {
  const result = diffLines(left, right, options);
  return result.ok ? result.lines.map((line) => `${line.kind[0]}:${line.text}`) : [result.reason];
};

describe("diffLines", () => {
  it("calls two identical texts identical", () => {
    expect(kinds("a\nb", "a\nb")).toEqual(["s:a", "s:b"]);
  });

  it("sees added lines", () => {
    expect(kinds("a\nc", "a\nb\nc")).toEqual(["s:a", "a:b", "s:c"]);
  });

  it("sees removed lines", () => {
    expect(kinds("a\nb\nc", "a\nc")).toEqual(["s:a", "r:b", "s:c"]);
  });

  it("counts added and removed lines", () => {
    const result = diffLines("a\nb", "a\nc\nd", plain);
    expect(result.ok && result.added).toBe(2);
    expect(result.ok && result.removed).toBe(1);
  });

  it("numbers lines per side", () => {
    const result = diffLines("a\nb", "a", plain);
    expect(result.ok && result.lines[1]).toEqual({
      kind: "remove",
      leftNo: 2,
      rightNo: null,
      text: "b",
    });
  });

  it("ignores whitespace when asked", () => {
    expect(kinds("a  b", "a b", { ignoreWhitespace: true, ignoreCase: false })).toEqual(["s:a  b"]);
  });

  it("ignores case when asked", () => {
    expect(kinds("Abc", "abc", { ignoreWhitespace: false, ignoreCase: true })).toEqual(["s:Abc"]);
  });

  // Trimming the identical head and tail is what makes the tool usable on real files: ten differing
  // lines between two 50-thousand-line files leave an LCS table of only 10×10.
  it("handles very long files when the differing part is small", () => {
    const head = Array.from({ length: 30_000 }, (_, i) => `dòng ${i}`).join("\n");
    const result = diffLines(`${head}\nX`, `${head}\nY`, plain);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.added).toBe(1);
    expect(result.removed).toBe(1);
  });

  it("refuses when the differing part exceeds the limit", () => {
    const left = Array.from({ length: 2100 }, (_, i) => `l${i}`).join("\n");
    const right = Array.from({ length: 2100 }, (_, i) => `r${i}`).join("\n");
    expect(diffLines(left, right, plain)).toEqual({ ok: false, reason: "tooLarge" });
  });

  // Locks down the invariant `computeLineSegments`/`buildSplitRows` rely on: a change cluster
  // always emits all its removes before its adds, never interleaved. If someone changes the
  // tie-break (`>=` → `>`) in `diffLines`, this test breaks first, instead of letting those two
  // functions quietly pair the wrong lines.
  it("a change cluster emits all removes before any adds", () => {
    expect(kinds("a\nb\nc", "a\nx\ny\nc")).toEqual(["s:a", "r:b", "a:x", "a:y", "s:c"]);
  });
});

describe("diffSegments", () => {
  it("highlights exactly the differing part in the middle", () => {
    const result = diffSegments("SELECT * FROM users WHERE id = 1", "SELECT * FROM users WHERE id = 2", plain);
    expect(result).toEqual({
      left: [
        { text: "SELECT * FROM users WHERE id = ", changed: false },
        { text: "1", changed: true },
      ],
      right: [
        { text: "SELECT * FROM users WHERE id = ", changed: false },
        { text: "2", changed: true },
      ],
    });
  });

  it("highlights a differing part at the start", () => {
    const result = diffSegments("foo bar baz", "quux bar baz", plain);
    expect(result).toEqual({
      left: [
        { text: "foo", changed: true },
        { text: " bar baz", changed: false },
      ],
      right: [
        { text: "quux", changed: true },
        { text: " bar baz", changed: false },
      ],
    });
  });

  it("highlights a differing part at the end", () => {
    const result = diffSegments("bar baz foo", "bar baz quux", plain);
    expect(result).toEqual({
      left: [
        { text: "bar baz ", changed: false },
        { text: "foo", changed: true },
      ],
      right: [
        { text: "bar baz ", changed: false },
        { text: "quux", changed: true },
      ],
    });
  });

  it("does not let head and tail overlap when one string is a prefix of the other", () => {
    const result = diffSegments("foo", "foobar", plain);
    expect(result).toEqual({
      left: [{ text: "foo", changed: false }],
      right: [
        { text: "foo", changed: false },
        { text: "bar", changed: true },
      ],
    });
  });

  it("returns null when two lines are too different — no highlighting a coincidence", () => {
    expect(diffSegments("SELECT id FROM a;", "DELETE FROM b WHERE x = 1;", plain)).toBeNull();
  });

  it("returns null when ignoreWhitespace is on", () => {
    expect(diffSegments("a  b", "a b c", { ignoreWhitespace: true, ignoreCase: false })).toBeNull();
  });

  it("respects ignoreCase when matching head/tail", () => {
    const result = diffSegments("Hello World", "hello there", { ignoreWhitespace: false, ignoreCase: true });
    expect(result).toEqual({
      left: [
        { text: "Hello ", changed: false },
        { text: "World", changed: true },
      ],
      right: [
        { text: "hello ", changed: false },
        { text: "there", changed: true },
      ],
    });
  });

  it("does not split a two-code-unit Unicode character", () => {
    const result = diffSegments("chào 😀 bạn", "chào 😀😀 bạn", plain);
    expect(result).not.toBeNull();
    // Joined back it must give exactly the original string — if a surrogate pair were split,
    // join() would give a broken character of a different length.
    expect(result!.left.map((s) => s.text).join("")).toBe("chào 😀 bạn");
    expect(result!.right.map((s) => s.text).join("")).toBe("chào 😀😀 bạn");
  });
});

describe("computeLineSegments", () => {
  it("attaches segments to the pairable remove/add pairs, leaving leftover lines bare", () => {
    const result = diffLines("a\nfoo\nc", "a\nfoobar\nc", plain);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const map = computeLineSegments(result.lines, plain);
    const removeLine = result.lines.find((l) => l.kind === "remove")!;
    const addLine = result.lines.find((l) => l.kind === "add")!;
    expect(map.get(removeLine)).toEqual([{ text: "foo", changed: false }]);
    expect(map.get(addLine)).toEqual([
      { text: "foo", changed: false },
      { text: "bar", changed: true },
    ]);
  });

  it("attaches nothing when a pair is not similar enough", () => {
    const result = diffLines("a\nSELECT id FROM a;\nc", "a\nDELETE FROM b WHERE x = 1;\nc", plain);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(computeLineSegments(result.lines, plain).size).toBe(0);
  });
});

describe("buildSplitRows", () => {
  it("puts a same line on one row on both sides", () => {
    const result = diffLines("a\nb", "a\nb", plain);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const rows = buildSplitRows(result.lines, plain);
    expect(rows).toEqual([
      {
        left: { kind: "same", no: 1, text: "a", segments: null },
        right: { kind: "same", no: 1, text: "a", segments: null },
      },
      {
        left: { kind: "same", no: 2, text: "b", segments: null },
        right: { kind: "same", no: 2, text: "b", segments: null },
      },
    ]);
  });

  it("pairs equal numbers of removes/adds into replaced rows", () => {
    const result = diffLines("foo", "foobar", plain);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const rows = buildSplitRows(result.lines, plain);
    expect(rows).toEqual([
      {
        left: { kind: "remove", no: 1, text: "foo", segments: [{ text: "foo", changed: false }] },
        right: {
          kind: "add",
          no: 1,
          text: "foobar",
          segments: [
            { text: "foo", changed: false },
            { text: "bar", changed: true },
          ],
        },
      },
    ]);
  });

  it("leaves the other side blank when remove and add counts differ", () => {
    const result = diffLines("a\nb", "a\nx\ny", plain);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const rows = buildSplitRows(result.lines, plain);
    // "a" is the same, "b" is paired with "x" (the first pair), "y" is a leftover add → the left
    // side is blank.
    expect(rows).toEqual([
      {
        left: { kind: "same", no: 1, text: "a", segments: null },
        right: { kind: "same", no: 1, text: "a", segments: null },
      },
      {
        left: { kind: "remove", no: 2, text: "b", segments: null },
        right: { kind: "add", no: 2, text: "x", segments: null },
      },
      { left: { kind: "blank" }, right: { kind: "add", no: 3, text: "y", segments: null } },
    ]);
  });
});
