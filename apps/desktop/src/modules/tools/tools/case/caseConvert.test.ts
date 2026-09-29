import { describe, expect, it } from "vitest";
import { convert, splitWords } from "./caseConvert";

describe("splitWords", () => {
  it("tách camelCase", () => {
    expect(splitWords("fooBar")).toEqual(["foo", "bar"]);
  });

  it("giữ cụm viết hoa liền nhau thành một từ", () => {
    expect(splitWords("getHTTPResponse")).toEqual(["get", "http", "response"]);
  });

  it("tách trước chữ số nhưng không tách bên trong cụm số-chữ", () => {
    expect(splitWords("user2FA")).toEqual(["user", "2fa"]);
  });

  it("coi mọi dấu ngăn là như nhau", () => {
    expect(splitWords("created_at")).toEqual(["created", "at"]);
    expect(splitWords("created-at")).toEqual(["created", "at"]);
    expect(splitWords("created at")).toEqual(["created", "at"]);
    expect(splitWords("created.at")).toEqual(["created", "at"]);
  });

  it("bỏ dấu ngăn thừa ở hai đầu và ở giữa", () => {
    expect(splitWords("__created___at__")).toEqual(["created", "at"]);
  });

  it("trả mảng rỗng cho chuỗi không có ký tự nào dùng được", () => {
    expect(splitWords("   ")).toEqual([]);
    expect(splitWords("")).toEqual([]);
  });

  /* Accented letters are letters, not separators. A split that only knows `a-zA-Z` would tear
     "có gì hot" into `c`, `g`, `hot` — every accented letter becoming a word boundary. */
  it("giữ nguyên chữ tiếng Việt thay vì coi dấu là ranh giới từ", () => {
    expect(splitWords("có gì hot")).toEqual(["có", "gì", "hot"]);
    expect(splitWords("Xin chào bạn")).toEqual(["xin", "chào", "bạn"]);
    expect(splitWords("tên_người_dùng")).toEqual(["tên", "người", "dùng"]);
  });

  it("tách được camelCase có dấu", () => {
    expect(splitWords("địaChỉNhà")).toEqual(["địa", "chỉ", "nhà"]);
  });

  it("giữ được chữ của bảng chữ cái khác", () => {
    expect(splitWords("städteListe")).toEqual(["städte", "liste"]);
    expect(splitWords("日本語 test")).toEqual(["日本語", "test"]);
  });
});

describe("convert", () => {
  const input = "created_at";

  it("đổi sang từng kiểu", () => {
    expect(convert(input, "camel")).toBe("createdAt");
    expect(convert(input, "snake")).toBe("created_at");
    expect(convert(input, "kebab")).toBe("created-at");
    expect(convert(input, "pascal")).toBe("CreatedAt");
    expect(convert(input, "constant")).toBe("CREATED_AT");
    expect(convert(input, "dot")).toBe("created.at");
    expect(convert(input, "title")).toBe("Created At");
  });

  it("trả lại nguyên dòng khi không tách được từ nào", () => {
    expect(convert("   ", "camel")).toBe("   ");
  });

  it("đổi được cả tên có dấu", () => {
    expect(convert("có gì hot", "snake")).toBe("có_gì_hot");
    expect(convert("có gì hot", "camel")).toBe("cóGìHot");
    expect(convert("có gì hot", "pascal")).toBe("CóGìHot");
    expect(convert("có gì hot", "constant")).toBe("CÓ_GÌ_HOT");
    expect(convert("có gì hot", "title")).toBe("Có Gì Hot");
  });
});
