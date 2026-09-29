import { describe, expect, it } from "vitest";
import { md5 } from "./md5";

const of = (text: string) => md5(new TextEncoder().encode(text));

describe("md5", () => {
  // The standard test vectors from appendix A.5 of RFC 1321.
  it("khớp bộ vector của RFC 1321", () => {
    expect(of("")).toBe("d41d8cd98f00b204e9800998ecf8427e");
    expect(of("a")).toBe("0cc175b9c0f1b6a831c399e269772661");
    expect(of("abc")).toBe("900150983cd24fb0d6963f7d28e17f72");
    expect(of("message digest")).toBe("f96b697d7cb7938d525a2f31aaf161d0");
    expect(of("abcdefghijklmnopqrstuvwxyz")).toBe("c3fcd3d76192e4007dfb496cca67e13b");
    expect(
      of("12345678901234567890123456789012345678901234567890123456789012345678901234567890"),
    ).toBe("57edf4a22be3c955ac49da2e2107b67a");
  });

  it("băm đúng qua ranh giới khối 64 byte", () => {
    expect(of("a".repeat(64))).toBe("014842d480b571495a4a0363793f7367");
    expect(of("a".repeat(56))).toBe("3b0c8ac703f828b04c6c197006d17218");
  });

  it("băm theo byte UTF-8, không theo mã ký tự", () => {
    // "Xin chào" has one non-ASCII character, so this 8-character string is 9 bytes.
    expect(new TextEncoder().encode("Xin chào")).toHaveLength(9);
    expect(of("Xin chào")).toHaveLength(32);
  });
});
