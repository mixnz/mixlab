import { describe, expect, it } from "vitest";
import { base64ToText, hashText, hexToText, textToBase64, textToHex } from "./encode";

describe("base64", () => {
  it("đi và về với ASCII", () => {
    expect(textToBase64("hello", false)).toBe("aGVsbG8=");
    expect(base64ToText("aGVsbG8=")).toBe("hello");
  });

  it("đi qua UTF-8, không qua `btoa` trần — dán tiếng Việt vào là thấy ngay", () => {
    const encoded = textToBase64("Xin chào", false);
    expect(base64ToText(encoded)).toBe("Xin chào");
    // 9 UTF-8 bytes become 12 base64 characters, not 8 characters becoming 12.
    expect(encoded).toHaveLength(12);
  });

  it("biến thể url-safe đổi +/ thành -_ và bỏ đệm", () => {
    expect(textToBase64("ÿÿÿ", true)).not.toMatch(/[+/=]/);
    expect(base64ToText(textToBase64("ÿÿÿ", true))).toBe("ÿÿÿ");
  });

  it("ném lỗi cho base64 hỏng", () => {
    expect(() => base64ToText("không phải base64!")).toThrow();
  });
});

describe("hex", () => {
  it("đi và về, có và không có dấu cách", () => {
    expect(textToHex("abc", false)).toBe("616263");
    expect(textToHex("abc", true)).toBe("61 62 63");
    expect(hexToText("616263")).toBe("abc");
    expect(hexToText("61 62 63")).toBe("abc");
  });

  it("ném lỗi khi số chữ hex lẻ hoặc có ký tự lạ", () => {
    expect(() => hexToText("61626")).toThrow();
    expect(() => hexToText("61zz63")).toThrow();
  });
});

describe("hashText", () => {
  it("dùng md5 tự viết cho MD5 và Web Crypto cho phần còn lại", async () => {
    await expect(hashText("abc", "MD5")).resolves.toBe("900150983cd24fb0d6963f7d28e17f72");
    await expect(hashText("abc", "SHA-1")).resolves.toBe(
      "a9993e364706816aba3e25717850c26c9cd0d89d",
    );
    await expect(hashText("abc", "SHA-256")).resolves.toBe(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
  });
});
