import { describe, expect, it } from "vitest";
import { parseCsvRows, rowsToObjects, toCsv } from "./csv";

describe("parseCsvRows", () => {
  it("splits on the separator", () => {
    expect(parseCsvRows("a,b\n1,2", ",")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  // The whole reason not to use `split(",")`.
  it("keeps a separator inside double quotes", () => {
    expect(parseCsvRows('a,b\n"x,y",2', ",")).toEqual([
      ["a", "b"],
      ["x,y", "2"],
    ]);
  });

  it("understands a doubled double quote as one double quote", () => {
    expect(parseCsvRows('a\n"nói ""xin chào"""', ",")).toEqual([["a"], ['nói "xin chào"']]);
  });

  it("keeps a newline inside double quotes", () => {
    expect(parseCsvRows('a,b\n"hai\ndòng",2', ",")).toEqual([
      ["a", "b"],
      ["hai\ndòng", "2"],
    ]);
  });

  it("treats CRLF as one newline", () => {
    expect(parseCsvRows("a,b\r\n1,2\r\n", ",")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  it("keeps empty fields at the start, middle and end of a line", () => {
    expect(parseCsvRows(",a,,b,", ",")).toEqual([["", "a", "", "b", ""]]);
  });

  it("does not produce an extra row from a trailing newline", () => {
    expect(parseCsvRows("a\n1\n", ",")).toEqual([["a"], ["1"]]);
  });

  it("accepts separators other than a comma", () => {
    expect(parseCsvRows("a;b\n1;2", ";")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });
});

describe("rowsToObjects", () => {
  it("takes the first row as column names", () => {
    expect(
      rowsToObjects([
        ["id", "name"],
        ["1", "An"],
      ]),
    ).toEqual([{ id: "1", name: "An" }]);
  });

  it("fills empty cells for rows shorter than the header", () => {
    expect(rowsToObjects([["a", "b"], ["1"]])).toEqual([{ a: "1", b: "" }]);
  });

  it("skips entirely empty rows", () => {
    expect(rowsToObjects([["a"], [""], ["1"]])).toEqual([{ a: "1" }]);
  });

  // Guessing types loses the leading zero of postal codes, silently, and it cannot be recovered.
  it("keeps every value a string, even ones that look like numbers", () => {
    expect(
      rowsToObjects([
        ["zip", "ok"],
        ["007", "true"],
      ]),
    ).toEqual([{ zip: "007", ok: "true" }]);
  });
});

describe("toCsv", () => {
  it("takes the union of keys as columns, in order of first appearance", () => {
    expect(toCsv([{ b: 1 }, { a: 2 }], ",", true)).toBe("b,a\n1,\n,2");
  });

  it("quotes values containing a separator, double quote or newline", () => {
    expect(toCsv([{ a: "x,y", b: 'nói "chào"', c: "hai\ndòng" }], ",", false)).toBe(
      '"x,y","nói ""chào""","hai\ndòng"',
    );
  });

  it("prints null as an empty cell and objects as JSON", () => {
    expect(toCsv([{ a: null, b: { x: 1 } }], ",", false)).toBe(',"{""x"":1}"');
  });

  it("omits the header row when not needed", () => {
    expect(toCsv([{ a: 1 }], ",", false)).toBe("1");
  });
});
