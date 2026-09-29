import { describe, expect, it } from "vitest";
import { parseCsvRows, rowsToObjects, toCsv } from "./csv";

describe("parseCsvRows", () => {
  it("tách theo dấu phân cách", () => {
    expect(parseCsvRows("a,b\n1,2", ",")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  // The whole reason not to use `split(",")`.
  it("giữ dấu phân cách nằm trong ngoặc kép", () => {
    expect(parseCsvRows('a,b\n"x,y",2', ",")).toEqual([
      ["a", "b"],
      ["x,y", "2"],
    ]);
  });

  it("hiểu ngoặc kép đôi là một dấu ngoặc kép", () => {
    expect(parseCsvRows('a\n"nói ""xin chào"""', ",")).toEqual([["a"], ['nói "xin chào"']]);
  });

  it("giữ xuống dòng nằm trong ngoặc kép", () => {
    expect(parseCsvRows('a,b\n"hai\ndòng",2', ",")).toEqual([
      ["a", "b"],
      ["hai\ndòng", "2"],
    ]);
  });

  it("nuốt CRLF như một lần xuống dòng", () => {
    expect(parseCsvRows("a,b\r\n1,2\r\n", ",")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });

  it("giữ trường rỗng ở đầu, giữa và cuối dòng", () => {
    expect(parseCsvRows(",a,,b,", ",")).toEqual([["", "a", "", "b", ""]]);
  });

  it("không đẻ ra một dòng thừa vì xuống dòng cuối file", () => {
    expect(parseCsvRows("a\n1\n", ",")).toEqual([["a"], ["1"]]);
  });

  it("nhận dấu phân cách khác dấu phẩy", () => {
    expect(parseCsvRows("a;b\n1;2", ";")).toEqual([
      ["a", "b"],
      ["1", "2"],
    ]);
  });
});

describe("rowsToObjects", () => {
  it("lấy dòng đầu làm tên cột", () => {
    expect(
      rowsToObjects([
        ["id", "name"],
        ["1", "An"],
      ]),
    ).toEqual([{ id: "1", name: "An" }]);
  });

  it("bù ô rỗng cho dòng ngắn hơn tiêu đề", () => {
    expect(rowsToObjects([["a", "b"], ["1"]])).toEqual([{ a: "1", b: "" }]);
  });

  it("bỏ dòng trống hoàn toàn", () => {
    expect(rowsToObjects([["a"], [""], ["1"]])).toEqual([{ a: "1" }]);
  });

  // Guessing types loses the leading zero of postal codes, silently, and it cannot be recovered.
  it("để mọi giá trị là chuỗi, kể cả thứ trông như số", () => {
    expect(
      rowsToObjects([
        ["zip", "ok"],
        ["007", "true"],
      ]),
    ).toEqual([{ zip: "007", ok: "true" }]);
  });
});

describe("toCsv", () => {
  it("lấy hợp các khoá làm cột, theo thứ tự xuất hiện lần đầu", () => {
    expect(toCsv([{ b: 1 }, { a: 2 }], ",", true)).toBe("b,a\n1,\n,2");
  });

  it("bọc ngoặc khi giá trị có dấu phân cách, ngoặc kép hoặc xuống dòng", () => {
    expect(toCsv([{ a: "x,y", b: 'nói "chào"', c: "hai\ndòng" }], ",", false)).toBe(
      '"x,y","nói ""chào""","hai\ndòng"',
    );
  });

  it("in null thành ô rỗng và object thành JSON", () => {
    expect(toCsv([{ a: null, b: { x: 1 } }], ",", false)).toBe(',"{""x"":1}"');
  });

  it("bỏ dòng tiêu đề khi không cần", () => {
    expect(toCsv([{ a: 1 }], ",", false)).toBe("1");
  });
});
