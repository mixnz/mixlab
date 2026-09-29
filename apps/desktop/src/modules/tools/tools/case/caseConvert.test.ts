import { describe, expect, it } from "vitest";
import { convert, splitWords } from "./caseConvert";

describe("splitWords", () => {
  it("splits camelCase", () => {
    expect(splitWords("fooBar")).toEqual(["foo", "bar"]);
  });

  it("keeps an uppercase run together as one word", () => {
    expect(splitWords("getHTTPResponse")).toEqual(["get", "http", "response"]);
  });

  it("splits before digits but not inside a digit-letter run", () => {
    expect(splitWords("user2FA")).toEqual(["user", "2fa"]);
  });

  it("treats every separator the same", () => {
    expect(splitWords("created_at")).toEqual(["created", "at"]);
    expect(splitWords("created-at")).toEqual(["created", "at"]);
    expect(splitWords("created at")).toEqual(["created", "at"]);
    expect(splitWords("created.at")).toEqual(["created", "at"]);
  });

  it("drops extra separators at both ends and in the middle", () => {
    expect(splitWords("__created___at__")).toEqual(["created", "at"]);
  });

  it("returns an empty array for a string with no usable characters", () => {
    expect(splitWords("   ")).toEqual([]);
    expect(splitWords("")).toEqual([]);
  });

  /* Accented letters are letters, not separators. A split that only knows `a-zA-Z` would tear
     "có gì hot" into `c`, `g`, `hot` — every accented letter becoming a word boundary. */
  it("keeps Vietnamese letters instead of treating accents as word boundaries", () => {
    expect(splitWords("có gì hot")).toEqual(["có", "gì", "hot"]);
    expect(splitWords("Xin chào bạn")).toEqual(["xin", "chào", "bạn"]);
    expect(splitWords("tên_người_dùng")).toEqual(["tên", "người", "dùng"]);
  });

  it("splits accented camelCase", () => {
    expect(splitWords("địaChỉNhà")).toEqual(["địa", "chỉ", "nhà"]);
  });

  it("keeps letters of other alphabets", () => {
    expect(splitWords("städteListe")).toEqual(["städte", "liste"]);
    expect(splitWords("日本語 test")).toEqual(["日本語", "test"]);
  });
});

describe("convert", () => {
  const input = "created_at";

  it("converts to each style", () => {
    expect(convert(input, "camel")).toBe("createdAt");
    expect(convert(input, "snake")).toBe("created_at");
    expect(convert(input, "kebab")).toBe("created-at");
    expect(convert(input, "pascal")).toBe("CreatedAt");
    expect(convert(input, "constant")).toBe("CREATED_AT");
    expect(convert(input, "dot")).toBe("created.at");
    expect(convert(input, "title")).toBe("Created At");
  });

  it("returns the line as is when no words can be split out", () => {
    expect(convert("   ", "camel")).toBe("   ");
  });

  it("converts accented names too", () => {
    expect(convert("có gì hot", "snake")).toBe("có_gì_hot");
    expect(convert("có gì hot", "camel")).toBe("cóGìHot");
    expect(convert("có gì hot", "pascal")).toBe("CóGìHot");
    expect(convert("có gì hot", "constant")).toBe("CÓ_GÌ_HOT");
    expect(convert("có gì hot", "title")).toBe("Có Gì Hot");
  });
});
