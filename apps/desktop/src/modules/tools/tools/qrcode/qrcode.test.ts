import { describe, expect, it } from "vitest";
import { encodeQr } from "./qrcode";

describe("encodeQr", () => {
  it("produces a square grid for short text", () => {
    const grid = encodeQr("hello", "M");
    expect(grid).not.toBeNull();
    expect(grid!.size).toBeGreaterThanOrEqual(21);
    expect((grid!.size - 21) % 4).toBe(0);
  });

  it("the top-left corner is always dark — every QR's finder pattern", () => {
    const grid = encodeQr("hello", "M")!;
    expect(grid.isDark(0, 0)).toBe(true);
  });

  it("text beyond QR capacity (even version 40) returns null instead of throwing", () => {
    const grid = encodeQr("a".repeat(5000), "H");
    expect(grid).toBeNull();
  });

  it("longer text needs a version (size) greater than or equal", () => {
    const small = encodeQr("hi", "M")!;
    const big = encodeQr("hello world, this is a much longer piece of text to encode", "M")!;
    expect(big.size).toBeGreaterThanOrEqual(small.size);
  });
});
