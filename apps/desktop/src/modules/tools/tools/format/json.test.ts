import { describe, expect, it } from "vitest";
import { formatJson, minifyJson } from "./json";

const out = (result: ReturnType<typeof formatJson>): string => (result.ok ? result.output : "");

describe("formatJson", () => {
  // This is why this whole file exists: the four values below break when passed through
  // `JSON.parse` + `stringify`, silently, and the user copies away a wrong id.
  it("keeps large numbers, key order, trailing zeros and escapes intact", () => {
    const source = '{"2":"a","1":"b","id":1787875200123456789,"price":1.50,"c":"\\u0041"}';
    const text = out(formatJson(source, "  "));

    expect(text).toContain('"id": 1787875200123456789');
    expect(text).toContain('"price": 1.50');
    expect(text).toContain('"c": "\\u0041"');
    expect(text.indexOf('"2"')).toBeLessThan(text.indexOf('"1"'));
  });

  it("prints nesting with the chosen indentation", () => {
    expect(formatJson('{"a":{"b":[1,2]}}', "  ")).toEqual({
      ok: true,
      output: '{\n  "a": {\n    "b": [\n      1,\n      2\n    ]\n  }\n}',
    });
  });

  it("prints empty arrays and empty objects compactly on one line", () => {
    expect(formatJson('{"a":[],"b":{}}', "  ")).toEqual({
      ok: true,
      output: '{\n  "a": [],\n  "b": {}\n}',
    });
  });

  it("accepts a tab as indentation", () => {
    expect(formatJson('{"a":1}', "\t")).toEqual({ ok: true, output: '{\n\t"a": 1\n}' });
  });

  it("does not touch whitespace inside strings", () => {
    expect(out(formatJson('{"a":"x  y"}', "  "))).toContain('"x  y"');
  });
});

describe("minifyJson", () => {
  it("removes all whitespace outside strings", () => {
    expect(minifyJson('{\n  "a": [1, 2],\n  "b": "x  y"\n}')).toEqual({
      ok: true,
      output: '{"a":[1,2],"b":"x  y"}',
    });
  });
});

describe("syntax errors", () => {
  it("points at the exact line and column of a trailing comma", () => {
    const result = formatJson('{\n  "a": 1,\n}', "  ");
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.error.line).toBe(3);
    expect(result.error.column).toBe(1);
  });

  it("catches an unquoted key", () => {
    expect(formatJson("{a:1}", "  ").ok).toBe(false);
  });

  it("catches an unclosed string", () => {
    expect(formatJson('{"a":"x}', "  ").ok).toBe(false);
  });

  it("catches trailing characters after the value", () => {
    expect(minifyJson('{"a":1} rác').ok).toBe(false);
  });

  it("catches empty input", () => {
    expect(minifyJson("   ").ok).toBe(false);
  });
});
