import { describe, expect, it } from "vitest";
import { formatJson, minifyJson } from "./json";

const out = (result: ReturnType<typeof formatJson>): string => (result.ok ? result.output : "");

describe("formatJson", () => {
  // This is why this whole file exists: the four values below break when passed through
  // `JSON.parse` + `stringify`, silently, and the user copies away a wrong id.
  it("giữ nguyên số lớn, thứ tự khoá, số 0 thừa và escape", () => {
    const source = '{"2":"a","1":"b","id":1787875200123456789,"price":1.50,"c":"\\u0041"}';
    const text = out(formatJson(source, "  "));

    expect(text).toContain('"id": 1787875200123456789');
    expect(text).toContain('"price": 1.50');
    expect(text).toContain('"c": "\\u0041"');
    expect(text.indexOf('"2"')).toBeLessThan(text.indexOf('"1"'));
  });

  it("in lồng nhau theo thụt lề đã chọn", () => {
    expect(formatJson('{"a":{"b":[1,2]}}', "  ")).toEqual({
      ok: true,
      output: '{\n  "a": {\n    "b": [\n      1,\n      2\n    ]\n  }\n}',
    });
  });

  it("in mảng rỗng và object rỗng gọn trên một dòng", () => {
    expect(formatJson('{"a":[],"b":{}}', "  ")).toEqual({
      ok: true,
      output: '{\n  "a": [],\n  "b": {}\n}',
    });
  });

  it("nhận tab làm thụt lề", () => {
    expect(formatJson('{"a":1}', "\t")).toEqual({ ok: true, output: '{\n\t"a": 1\n}' });
  });

  it("không đụng vào khoảng trắng bên trong chuỗi", () => {
    expect(out(formatJson('{"a":"x  y"}', "  "))).toContain('"x  y"');
  });
});

describe("minifyJson", () => {
  it("bỏ hết khoảng trắng ngoài chuỗi", () => {
    expect(minifyJson('{\n  "a": [1, 2],\n  "b": "x  y"\n}')).toEqual({
      ok: true,
      output: '{"a":[1,2],"b":"x  y"}',
    });
  });
});

describe("lỗi cú pháp", () => {
  it("chỉ đúng dòng và cột của dấu phẩy thừa", () => {
    const result = formatJson('{\n  "a": 1,\n}', "  ");
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.error.line).toBe(3);
    expect(result.error.column).toBe(1);
  });

  it("bắt khoá không có ngoặc kép", () => {
    expect(formatJson("{a:1}", "  ").ok).toBe(false);
  });

  it("bắt chuỗi chưa đóng", () => {
    expect(formatJson('{"a":"x}', "  ").ok).toBe(false);
  });

  it("bắt ký tự thừa sau giá trị", () => {
    expect(minifyJson('{"a":1} rác').ok).toBe(false);
  });

  it("bắt đầu vào rỗng", () => {
    expect(minifyJson("   ").ok).toBe(false);
  });
});
