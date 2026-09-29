import { describe, expect, it } from "vitest";
import { detectBase, formatOutputs, parseValue } from "./radix";

describe("detectBase", () => {
  it("reads the 0x prefix as hex", () => {
    expect(detectBase("0xFF")).toBe("hex");
  });

  it("reads the 0b prefix as binary", () => {
    expect(detectBase("0b1010")).toBe("bin");
  });

  it("reads the 0o prefix as octal", () => {
    expect(detectBase("0o17")).toBe("oct");
  });

  it("reads all digits with no prefix as decimal", () => {
    expect(detectBase("255")).toBe("dec");
  });

  it("keeps the minus sign before checking the prefix", () => {
    expect(detectBase("-0xFF")).toBe("hex");
  });

  it("returns null when it cannot be read", () => {
    expect(detectBase("hello")).toBeNull();
    expect(detectBase("")).toBeNull();
  });
});

describe("parseValue", () => {
  it("reads decimal", () => {
    expect(parseValue("255", "dec")).toBe(255n);
  });

  it("reads hex with or without a prefix", () => {
    expect(parseValue("0xff", "hex")).toBe(255n);
    expect(parseValue("ff", "hex")).toBe(255n);
  });

  it("reads prefixed binary", () => {
    expect(parseValue("0b1010", "bin")).toBe(10n);
  });

  it("reads prefixed octal", () => {
    expect(parseValue("0o17", "oct")).toBe(15n);
  });

  it("reads negative numbers in decimal", () => {
    expect(parseValue("-42", "dec")).toBe(-42n);
  });

  // Negative numbers only have a convention in decimal — hex/oct/bin do not do two's complement
  // here.
  it("does not read negative numbers in hex/oct/bin", () => {
    expect(parseValue("-0xFF", "hex")).toBeNull();
    expect(parseValue("-11", "bin")).toBeNull();
    expect(parseValue("-17", "oct")).toBeNull();
  });

  // bigint/snowflake-style IDs exceed Number.MAX_SAFE_INTEGER — this is why BigInt is used
  // throughout.
  it("reads numbers larger than Number.MAX_SAFE_INTEGER", () => {
    expect(parseValue("9223372036854775807", "dec")).toBe(9223372036854775807n);
  });

  it("returns null for a digit outside the base", () => {
    expect(parseValue("102", "bin")).toBeNull();
    expect(parseValue("8", "oct")).toBeNull();
    expect(parseValue("g", "hex")).toBeNull();
  });

  it("returns null for an empty string or a lone minus sign", () => {
    expect(parseValue("", "dec")).toBeNull();
    expect(parseValue("-", "dec")).toBeNull();
  });
});

describe("formatOutputs", () => {
  it("prints all four bases correctly", () => {
    expect(formatOutputs(255n)).toEqual({
      bin: "1111 1111",
      oct: "377",
      dec: "255",
      hex: "0xff",
    });
  });

  it("groups binary in 4 bits from the right, remainder first", () => {
    expect(formatOutputs(10n).bin).toBe("1010");
    expect(formatOutputs(5n).bin).toBe("101");
    expect(formatOutputs(256n).bin).toBe("1 0000 0000");
  });

  it("keeps the minus sign in all four bases", () => {
    expect(formatOutputs(-255n)).toEqual({
      bin: "-1111 1111",
      oct: "-377",
      dec: "-255",
      hex: "-0xff",
    });
  });

  it("zero prints as 0 in every base", () => {
    expect(formatOutputs(0n)).toEqual({ bin: "0", oct: "0", dec: "0", hex: "0x0" });
  });
});
