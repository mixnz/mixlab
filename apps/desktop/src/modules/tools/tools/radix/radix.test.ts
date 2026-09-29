import { describe, expect, it } from "vitest";
import { detectBase, formatOutputs, parseValue } from "./radix";

describe("detectBase", () => {
  it("nhận tiền tố 0x là hex", () => {
    expect(detectBase("0xFF")).toBe("hex");
  });

  it("nhận tiền tố 0b là nhị phân", () => {
    expect(detectBase("0b1010")).toBe("bin");
  });

  it("nhận tiền tố 0o là bát phân", () => {
    expect(detectBase("0o17")).toBe("oct");
  });

  it("toàn chữ số không tiền tố thì đọc là thập phân", () => {
    expect(detectBase("255")).toBe("dec");
  });

  it("giữ dấu trừ trước khi soi tiền tố", () => {
    expect(detectBase("-0xFF")).toBe("hex");
  });

  it("không đọc được thì trả null", () => {
    expect(detectBase("hello")).toBeNull();
    expect(detectBase("")).toBeNull();
  });
});

describe("parseValue", () => {
  it("đọc thập phân", () => {
    expect(parseValue("255", "dec")).toBe(255n);
  });

  it("đọc hex có hoặc không tiền tố", () => {
    expect(parseValue("0xff", "hex")).toBe(255n);
    expect(parseValue("ff", "hex")).toBe(255n);
  });

  it("đọc nhị phân có tiền tố", () => {
    expect(parseValue("0b1010", "bin")).toBe(10n);
  });

  it("đọc bát phân có tiền tố", () => {
    expect(parseValue("0o17", "oct")).toBe(15n);
  });

  it("đọc được số âm ở thập phân", () => {
    expect(parseValue("-42", "dec")).toBe(-42n);
  });

  // Negative numbers only have a convention in decimal — hex/oct/bin do not do two's complement
  // here.
  it("không đọc số âm ở hex/oct/bin", () => {
    expect(parseValue("-0xFF", "hex")).toBeNull();
    expect(parseValue("-11", "bin")).toBeNull();
    expect(parseValue("-17", "oct")).toBeNull();
  });

  // bigint/snowflake-style IDs exceed Number.MAX_SAFE_INTEGER — this is why BigInt is used
  // throughout.
  it("đọc được số lớn hơn Number.MAX_SAFE_INTEGER", () => {
    expect(parseValue("9223372036854775807", "dec")).toBe(9223372036854775807n);
  });

  it("chữ số sai hệ thì trả null", () => {
    expect(parseValue("102", "bin")).toBeNull();
    expect(parseValue("8", "oct")).toBeNull();
    expect(parseValue("g", "hex")).toBeNull();
  });

  it("chuỗi rỗng hoặc chỉ có dấu trừ thì trả null", () => {
    expect(parseValue("", "dec")).toBeNull();
    expect(parseValue("-", "dec")).toBeNull();
  });
});

describe("formatOutputs", () => {
  it("in đúng cả bốn hệ", () => {
    expect(formatOutputs(255n)).toEqual({
      bin: "1111 1111",
      oct: "377",
      dec: "255",
      hex: "0xff",
    });
  });

  it("nhóm nhị phân theo 4 bit từ bên phải, phần lẻ ở đầu", () => {
    expect(formatOutputs(10n).bin).toBe("1010");
    expect(formatOutputs(5n).bin).toBe("101");
    expect(formatOutputs(256n).bin).toBe("1 0000 0000");
  });

  it("giữ dấu trừ ở cả bốn hệ", () => {
    expect(formatOutputs(-255n)).toEqual({
      bin: "-1111 1111",
      oct: "-377",
      dec: "-255",
      hex: "-0xff",
    });
  });

  it("số 0 in ra 0 ở mọi hệ", () => {
    expect(formatOutputs(0n)).toEqual({ bin: "0", oct: "0", dec: "0", hex: "0x0" });
  });
});
